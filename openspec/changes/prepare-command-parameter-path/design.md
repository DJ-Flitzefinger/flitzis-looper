# Design: Command and parameter path preparation

## Queue split

The current control-to-audio ring remains the ordered command queue for discrete events and
stateful operations where relative order matters. Playback triggers, stop commands, load/publish
messages, transport/mode changes, stem mode/mask changes, loop-region updates, pause/resume, and
unload stay on this queue.

Fast scalar parameter updates use a second bounded ring with the same producer-side nonblocking
semantics. Initial parameter coverage is:

- master volume,
- global speed,
- master BPM,
- per-pad BPM,
- per-pad gain,
- per-pad EQ.

Loop-region updates remain ordered commands for this slice because current Python playback and
Rust direct MIDI dispatch rely on loop-region state being applied before a paired trigger.

## Callback drain order

The audio callback drains the ordered command queue first, within the existing fixed command
budget. It then drains a separately bounded number of parameter messages from the parameter queue.
This prevents parameter bursts from occupying command queue capacity or delaying trigger/stop
messages behind a backlog of continuous controls.

## Parameter coalescing

Parameter messages drained during one callback invocation are compacted into fixed-size stack/local
state before being applied to the mixer. If multiple updates target the same parameter identity,
only the latest drained value is applied.

This avoids repeated coefficient or scalar updates for high-rate controls within one callback
budget. Future DSP parameters should use the same parameter path and then apply smoothing in the
audio-side DSP parameter state before sample processing.

## Queue-full semantics

Ordered command queue full remains an error for command APIs that already report ring-full
failure. Best-effort parameter setters may drop the newest update when the parameter queue is full;
future updates for the same parameter can replace it through the next accepted parameter message.

The audio callback never blocks waiting for either queue and never retries a full queue from the
audio thread.

## Direct MIDI dispatch

Rust input mapping remains outside the audio callback. Existing direct trigger dispatch currently
publishes a loop-region update followed by a play command. This slice makes that multi-message
publish all-or-nothing by checking command queue capacity while holding the existing producer
mutex. If there is not enough capacity for the whole sequence, the dispatcher sends none of it.

## Realtime constraints

The audio callback must still avoid disk I/O, JSON reads/writes, Python/GIL access, UI calls,
blocking locks or waits, logging, neural inference, plugin loading/scanning, unbounded loops,
heavy allocation, and long-running work.
