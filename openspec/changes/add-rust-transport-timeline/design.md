## Context
The current Rust audio callback drains control messages, applies them immediately to
`RtMixer`, renders one output buffer, and increments a local `frame_clock` used for
low-rate status messages. That is enough for responsive immediate triggering, but it does
not establish a global musical timeline that other Gen3 behavior can share.

The next Gen3 features need a single Rust authority for:

- output sample-frame time,
- transport master BPM,
- beat and bar phase,
- downbeat alignment,
- quantized pad starts/restarts,
- later phase-aware BPM lock and stem synchronization.

Python should remain the UI/control layer. It may request transport changes and display
state, but it must not own sample-accurate audio-thread timing.
Pads also must not own the masterclock: stopping the first-started or oldest active pad must
not reselect or invalidate the transport phase used by later quantized triggers.

## Goals
- Add a Rust-owned transport timeline advanced only by the audio callback's rendered output
  frames.
- Keep that transport timeline running from audio stream initialization, independent of active
  pad count and independent of trigger quantization state.
- Represent scheduler targets as absolute output-frame positions.
- Preserve existing `play_sample(id, velocity)` behavior by default.
- Make quantized pad triggers opt-in and deterministic while preserving source-side loop starts.
- Keep all audio callback work real-time safe: no blocking, no GIL, no disk I/O, no logging,
  no heap allocation, no neural inference, and no long-running work.
- Keep inter-thread communication compatible with the existing fixed-capacity `rtrb` model.
- Define scheduler-full behavior before implementation.

## Non-Goals
- Replacing the mixer or ring-buffer architecture.
- Real-time stem separation.
- Neural network inference in the audio callback.
- Disk I/O in the audio callback.
- Generating stems in this change.
- Publishing or processing unbounded beat-grid vectors inside the audio callback.

Later Gen3 stem work must be offline/cache-based and must only generate or replace stem
buffers for pads that are not currently playing. The audio callback may eventually mix
already prepared stem buffers, but it must not create them.

## Current Behavior To Preserve
- `AudioEngine.play_sample(id, velocity)` enqueues a fixed-size `PlaySample` control
  message.
- The audio callback starts or restarts the pad when it drains that message.
- MultiLoop disabled stops other active pads before starting the requested pad.
- MultiLoop enabled allows concurrent pads and retriggers only the selected pad.
- Missing samples and invalid mixer-side state are ignored safely.
- Ring-buffer-full errors are reported or dropped according to the existing API contract,
  without blocking the audio thread.

## Proposed Design

### Transport Timeline
Add a Rust transport state object owned by the audio thread. It should hold at least:

- `output_frame: u64`, the absolute sample-frame clock for rendered output frames.
- `sample_rate_hz`, copied from the CPAL stream configuration.
- `master_bpm: Option<f32>`, validated as finite and positive and initialized to a finite default
  so the masterclock exists before any pad is triggered.
- `beats_per_bar: u32`, initially fixed at 4 for 4/4 behavior.
- `downbeat_frame: u64`, the absolute output-frame anchor for bar phase.

At callback entry, `buffer_start_frame = output_frame`. After rendering `frames` output
frames, the transport advances to `output_frame + frames`.

Beat and bar phase are derived from `output_frame`, `sample_rate_hz`, `master_bpm`, and
`downbeat_frame`. If no valid master BPM exists, musical quantization is unavailable and
immediate playback remains available.

Pad playback is a client of this transport, not its owner. Pad starts, stops, pauses, retriggers,
unloads, pad BPM changes, and pad timing-metadata publication do not move `output_frame`,
`master_bpm`, or `downbeat_frame`. Transport BPM/phase changes require an explicit transport or
sync operation.

### Absolute Output-Frame Scheduler
Add a fixed-capacity scheduler owned by the audio thread. A scheduled event contains:

- `target_frame: u64`,
- a monotonic sequence number for stable ordering when multiple events share a frame,
- a fixed-size command payload, such as play, stop, stop-all, or transport update.

The scheduler must not allocate in the audio callback. It may use a fixed array, fixed ring,
or other bounded structure. The implementation should define a named capacity such as
`MAX_SCHEDULED_EVENTS`.

During a callback:

1. Drain control messages without blocking.
2. Convert immediate commands into `target_frame = buffer_start_frame`.
3. Convert quantized trigger requests into the current or next future selected transport grid target computed by
   the Rust transport.
4. Execute events due at or before `buffer_start_frame` before rendering the first frame.
5. Execute events that fall inside the current output buffer at their exact frame offset.
   This may require rendering the buffer in bounded sub-ranges between event offsets.
6. Leave future events in the scheduler for later callbacks.

Late events are not allowed to block or rewind output time. If `target_frame < buffer_start_frame`,
they execute at `buffer_start_frame`. Quantized trigger target selection avoids choosing past grid
boundaries; a click that missed the nearest previous gridline waits for the next future gridline
instead of seeking into the source loop.

### Quantized Pad Triggers
Default pad triggering remains immediate. Quantization is opt-in through future Python/UI
controls and fixed-size control messages.

The quantization model must support:

- disabled/immediate,
- `1/16`,
- `1/32`,
- `1/64`.

The default persisted grid step is `1/16`, while new projects still default the effective
enabled state to disabled/immediate. The minimum `1/64` step uses the same one-sixteenth-of-a-beat
unit as the loop editor's finest musical grid and snapping step.

When quantization is enabled, Rust computes the target frame from the permanent transport state and
selected grid step. A trigger exactly on a grid boundary may execute at the current output frame.
Otherwise it targets the next future matching grid boundary. Quantization only changes output
start time; the requested pad starts from its effective Loop Editor loop start, or sample start
when no loop region exists. The old late-click catch-up behavior that starts inside the loop is
out of scope for this corrected architecture.

Rust must not establish the masterclock from the first active pad or refresh it from whichever pad
is oldest/currently active as a side effect of quantized triggering. The same per-pad grid anchor
that the loop editor draws and snaps against is still published to Rust as pad timing metadata, but
that metadata does not redefine the permanent transport. Manual musical offsets are preserved:
triggering a second pad two bars later schedules that second pad two bars later on the global grid
rather than forcing it to the first pad's first beat or bar.

For MultiLoop disabled, the stop-other-pads action and the requested pad start must be one
scheduled transition at the same target frame. If that transition cannot be scheduled, the
engine must leave currently playing pads unchanged.

### Scheduler-Full Behavior
If the scheduler has no free event slot:

- the new scheduled request is rejected,
- existing scheduled events are not evicted,
- currently playing voices are not stopped as a side effect,
- no heap allocation, blocking wait, panic, logging, or Python/GIL access occurs,
- a best-effort audio-to-control diagnostic message may be emitted, but failure to emit
  that diagnostic must also be ignored safely.

For immediate compatibility commands, the implementation may bypass the scheduler or
schedule at `buffer_start_frame`, but it must retain the current deterministic behavior when
no quantization is requested.

### Beatgrid And Downbeat Integration
Analysis may produce BPM, beats, downbeats, and bars. That work remains outside the audio
callback. Before the audio thread uses the metadata, Python/Rust background code must convert
it into bounded, finite timing values suitable for fixed-size messages or preallocated
audio-thread state.

The transport and mixer should use the same onset fallback already used by loop-region
defaults:

1. first downbeat when available,
2. otherwise first beat when available,
3. otherwise `0.0` seconds.

The audio thread should receive enough per-pad timing metadata to align a pad's loop start,
beat phase, and bar phase against the global transport when quantized triggering or later
phase-aware BPM lock needs it. Full beat-grid vector processing, beat detection, and metadata
allocation do not belong in the callback.

### Message Passing Compatibility
The existing SPSC ring buffers remain the transport between Python/control code and the
audio callback. New transport and scheduler messages must be fixed-size, bounded, and safe
to drop or reject without blocking.

Python continues to call `AudioEngine` methods. The audio thread owns the consumer side of
control messages, the transport timeline, and the scheduler.

## Risks And Trade-offs
- Rendering sub-ranges within a callback adds mixer complexity. It is the cleanest way to
  execute events at frame offsets without changing the CPAL callback model.
- A fixed scheduler can fill during aggressive quantized input. Rejecting the newest event is
  simpler and safer than eviction because it avoids surprising changes to already accepted
  performance actions.
- Beatgrid metadata can be large. The design intentionally requires bounded summaries or
  preallocated state before audio-thread use.
- Phase-aware BPM lock should be built after the transport and scheduler are tested; the
  first implementation should not combine too many behavior changes.

## Open Questions
- Exact scheduler capacity. Start with a named constant and tests; 1024 is consistent with
  existing ring-buffer capacity, but a smaller capacity may be enough if justified.
- Whether scheduler-full diagnostics should be a new `AudioMessage` variant or only internal
  counters/state.
