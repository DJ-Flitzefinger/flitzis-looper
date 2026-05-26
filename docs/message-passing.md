# Message Passing

Flitzis Looper keeps the CPAL audio callback realtime-safe by communicating
with it through fixed-capacity, single-producer/single-consumer ring buffers.
Python never touches the ring buffers directly; it calls `AudioEngine` methods
which enqueue small Rust message values.

## Channels

The realtime bridge uses three SPSC ring buffers with capacity 1024:

- Command control -> audio: ordered events and state changes from Python or the
  Rust input dispatcher to the CPAL callback.
- Parameter control -> audio: high-rate scalar updates from Python control code
  to the CPAL callback.
- Audio -> control: small callback telemetry and test messages polled by Python.

The Rust MIDI input layer also uses bounded queues outside the CPAL callback.
Mapped MIDI playback can affect audio only by sending bounded command messages
or by reporting controller-owned actions back to Python.

## Ordered Commands

Ordered command messages are used where sequence matters or where the message
publishes a handle/state transition:

- `PlaySample`, `PlaySampleExclusive`, `StopSample`, `StopAll`
- `PauseSample`, `ResumeSample`
- `LoadSample`, `UnloadSample`
- `PublishPreparedStems`
- `SetPadLoopRegion`
- `SetTriggerQuantization`
- `SetBpmLock`, `SetKeyLock`, `SetKeyLockQuality`, `SetKeyLockSettings`
- `SetPadTimingMetadata`, `AnchorTransportPhaseFromPad`
- `SetStemMixMode`, `SetStemEnabledMask`
- `Ping`

Loop-region updates remain ordered because direct Rust MIDI trigger dispatch may
publish a loop-region update and a trigger as one all-or-nothing command
transaction.

## Fast Parameters

High-rate scalar values use the separate parameter ring:

- master volume
- global speed
- master BPM
- per-pad BPM
- per-pad gain
- per-pad EQ/DSP targets

The audio callback drains at most `MAX_PARAMETER_MESSAGES_PER_CALLBACK`
messages per invocation, currently `64`, and coalesces drained updates by
parameter identity before applying them. If several updates for one parameter
arrive in the same drain budget, only the latest drained target is applied.

## Callback Work Budget

The callback also drains at most `MAX_CONTROL_MESSAGES_PER_CALLBACK` ordered
commands per invocation, currently `64`. Messages beyond the per-callback
budget remain queued for later callbacks.

This keeps command bursts and controller sweeps from creating unbounded callback
work. If the parameter ring is full, best-effort parameter setters may drop the
newest update. Later accepted updates for the same parameter replace older
drained targets.

## Audio-To-Control Telemetry

The audio-to-control ring carries:

- `Pong`
- `SampleStarted`
- `SampleStopped`
- `PadPeak`
- `PadPlayhead`

`AppController.poll_runtime_events()` owns dispatching these messages to the
loader, playback, and metering controllers. UI rendering may request polling,
but it does not own telemetry mutation directly.

Telemetry is best-effort. Delayed or dropped audio messages may leave transient
`SessionState` projections stale until later telemetry or explicit controller
actions reconcile them. Durable `ProjectState` is not silently changed by
missing telemetry.

## Sample And Stem Publication

Decoded sample buffers and prepared stem buffers are not copied through ring
messages. Non-realtime workers create shared immutable handles, then publish
small descriptors and handles through fixed-size command messages.

When the callback removes, replaces, or rejects a loaded sample or prepared stem
set, large final handle drops are moved to a bounded non-audio retirement worker
instead of running in the callback.

## Realtime Safety Rules

The audio callback must not:

- acquire the Python GIL,
- read or write JSON,
- read, write, or scan files,
- decode audio,
- run Demucs or neural inference,
- own MIDI ports or keyboard polling,
- inspect mapping files or Learn state,
- load or scan plugins,
- log,
- block on locks or waits,
- perform unbounded loops or heavy allocation.

The callback may only consume bounded messages, update already-owned Rust state,
run the fixed-capacity scheduler, render already available buffers, process
fixed-size DSP state, and emit small telemetry messages.

## Direct Rust MIDI Dispatch

Rust MIDI capture runs outside the audio callback. It timestamps and normalizes
supported MIDI messages, resolves in-memory mapping snapshots, and may dispatch
only discrete audio-safe commands directly through the ordered command ring.
Controller-owned actions such as Tap BPM, stem masks, gain, EQ, master volume,
and speed are reported to Python as small event dictionaries and interpreted
outside the callback.

Future DSP parameter mappings must continue to derive bounded controller-owned
targets before crossing into the parameter ring and Rust-side smoothing.

## Related Specs And Docs

- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/play-samples/spec.md`
- `openspec/specs/load-audio-files/spec.md`
- `openspec/specs/per-pad-eq3/spec.md`
- `docs/audio-engine.md`
- `docs/audio-state-ownership.md`
- `docs/input-mapping-dsp-parameter-policy.md`
