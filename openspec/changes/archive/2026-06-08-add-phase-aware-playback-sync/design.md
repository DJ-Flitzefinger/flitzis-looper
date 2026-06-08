## Context
The active `add-rust-transport-timeline` change introduced the first Gen3 timing layer:

- an audio-thread-owned output-frame transport timeline,
- validated master BPM and 4/4 beat/bar phase helpers,
- a fixed-capacity scheduler for absolute output-frame events,
- low-level grid-step trigger quantization,
- atomic exclusive stop-all-then-play transitions,
- bounded per-pad phase anchors derived from analysis metadata.

Those anchors are stored in Rust, but they are not yet used to choose a pad's initial
playback frame. BPM lock also remains tempo-ratio based: it changes playback speed using
master BPM and pad BPM metadata, but it does not align downbeats.

The corrected trigger-quantization architecture keeps normal pad starts loop-start based.
Transport phase helpers and explicit phase-anchor requests remain useful for controlled sync
operations, but quantized human triggers must not seek into the source loop to catch up to a
gridline.

## Goals
- Keep quantized pad starts attached to the permanent Rust transport for output timing.
- Preserve the source-side loop-start invariant for every newly triggered pad.
- Keep non-quantized/immediate triggering behavior unchanged.
- Use bounded per-pad timing anchors already published to Rust.
- Allow explicit sync requests to anchor the Rust transport downbeat from a selected active pad
  when possible.
- Preserve one scheduled frame for MultiLoop-disabled stop-all/start transitions.
- Keep all audio callback work bounded and real-time safe.

## Non-Goals
- Continuous phase correction for already playing non-anchor voices.
- Time-stretch drift correction beyond the existing BPM-ratio matching behavior.
- New UI controls for trigger quantization.
- Full beat-grid vector processing in the audio callback.
- Stem generation or stem mixing.
- Late-click catch-up by seeking into the middle or end of a newly triggered source loop.
- Implicit masterclock selection from the first-started, oldest active, or currently playing pad.

## Proposed Design

### Phase Reference Model
Rust already has two pieces of phase state:

- Transport phase: `output_frame`, output sample rate, master BPM, 4 beats per bar, and
  `downbeat_frame`.
- Pad phase: effective pad BPM and one bounded `phase_anchor_frame` derived from the first
  downbeat, first beat, or zero.

For explicit sync features, Rust can compute the target transport phase at an arbitrary output
frame. With 4/4 behavior this is a value in beats within the bar:

```text
target_bar_phase_beats = transport.bar_phase_at(target_output_frame)
```

For the pad, Rust computes:

```text
pad_frames_per_beat = output_sample_rate * 60 / pad_bpm
desired_sample_frame = phase_anchor_frame + target_bar_phase_beats * pad_frames_per_beat
```

This calculation is retained as a bounded helper for explicit sync behavior and diagnostics. It is
not applied to normal quantized pad triggers in the corrected architecture. Newly triggered pads
start from the effective loop start, and quantization chooses only the output frame.

This model uses one bounded anchor and BPM metadata. It does not require full beat-grid
vectors or beat-detection work in the callback.

### Quantized Starts And Retriggers
When trigger quantization is disabled, `play_sample` and `play_sample_exclusive` retain the
current behavior: they start promptly at the effective loop start.

When trigger quantization is enabled with a selected grid step:

1. The audio thread computes the absolute output-frame target from the transport timeline.
2. The fixed-capacity scheduler stores the target and fixed-size command payload.
3. At execution time, Rust starts or restarts the pad at the existing effective loop start.

The scheduled event does not need to carry a source-frame phase descriptor for normal triggers.
If a future OpenSpec change reintroduces source-frame phase alignment as an explicit mode, that
mode must preserve fixed-size scheduling and must not become an implicit side effect of enabling
basic trigger quantization.

### MultiLoop-Disabled Atomic Transitions
For MultiLoop disabled playback, the stop-all and loop-start pad start remain one atomic
scheduled command at one output frame. If the scheduler rejects that command, currently
playing pads remain unchanged.

The target pad must still be validated before stopping active voices. A missing or invalid
target pad must not stop the current pad.

### BPM Lock Transport Phase Anchor
When BPM lock is enabled, Python tracks the selected anchor pad and derives a performance master
BPM for tempo-ratio matching. The audio thread now treats an accepted master-BPM parameter update
as the shared tempo for both mixer BPM-ratio matching and the permanent transport grid, preserving
the current transport bar phase at the callback's current output frame. A fixed-size request can
still explicitly ask the audio thread to anchor transport phase from a selected pad when possible;
that pad-derived phase sync is not implied by the master-BPM update.

The audio thread can anchor transport downbeat from a playing anchor pad when all are valid:

- the anchor pad id,
- master BPM,
- anchor pad BPM,
- anchor pad phase anchor,
- a currently active voice/playhead for the anchor pad,
- output sample rate.

Conceptually:

```text
pad_phase_beats = (current_pad_frame - phase_anchor_frame) / pad_frames_per_beat
pad_bar_phase_beats = pad_phase_beats modulo 4
downbeat_frame = current_output_frame - pad_bar_phase_beats * transport_frames_per_beat
```

If the calculated downbeat would be before frame zero, Rust can add whole bar durations until
the equivalent downbeat anchor is representable. The resulting transport phase is equivalent
for future quantized scheduling.

If the anchor pad is not playing or lacks valid timing data, BPM lock continues to provide
tempo-ratio matching without changing the transport downbeat anchor. Starting, stopping, pausing,
retriggering, or unloading pads does not implicitly invoke this anchor path.

### Real-Time Safety
The audio callback must continue to avoid:

- disk I/O,
- Python/GIL access,
- blocking locks or waits,
- logging,
- heap allocation,
- neural network inference,
- real-time stem separation,
- long-running work.

All calculations are scalar arithmetic over fixed-size mixer, transport, and scheduler state.
Any implementation that needs audio-thread allocation or blocking is a blocker.

## Risks And Trade-offs
- Source-frame phase alignment can surprise users when a custom loop region does not begin on the
  pad anchor. The corrected trigger path therefore preserves loop-start starts and limits
  phase-anchor behavior to explicit sync paths.
- BPM-lock anchoring from a currently playing pad is best-effort. If the anchor pad is not
  active, the engine should keep tempo matching rather than inventing phase.
- Active non-anchor pads may remain phase-offset until retriggered. Continuous correction is
  deliberately deferred to avoid a larger DSP change.

## Open Questions
- Whether a future UI should expose source-frame phase alignment separately from trigger
  quantization, with clear behavior and tests.
- Whether pad phase should remain based on bar phase only for all current grid subdivisions.
- Whether an audio-to-control diagnostic is useful when BPM-lock phase anchoring cannot be
  established from the selected pad.
