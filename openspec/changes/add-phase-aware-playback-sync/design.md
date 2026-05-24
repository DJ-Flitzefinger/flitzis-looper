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

## Goals
- Make quantized pad starts phase-aware when pad BPM and timing anchors are available.
- Keep non-quantized/immediate triggering behavior unchanged.
- Use bounded per-pad timing anchors already published to Rust.
- Anchor the Rust transport downbeat from the selected BPM-lock source pad when possible.
- Preserve one scheduled frame for MultiLoop-disabled stop-all/start transitions.
- Keep all audio callback work bounded and real-time safe.

## Non-Goals
- Continuous phase correction for already playing non-anchor voices.
- Time-stretch drift correction beyond the existing BPM-ratio matching behavior.
- New UI controls for trigger quantization.
- Full beat-grid vector processing in the audio callback.
- Stem generation or stem mixing.

## Proposed Design

### Phase Reference Model
Rust already has two pieces of phase state:

- Transport phase: `output_frame`, output sample rate, master BPM, 4 beats per bar, and
  `downbeat_frame`.
- Pad phase: effective pad BPM and one bounded `phase_anchor_frame` derived from the first
  downbeat, first beat, or zero.

For phase-aware starts, Rust computes the target transport phase at the scheduled output
frame. With 4/4 behavior this is a value in beats within the bar:

```text
target_bar_phase_beats = transport.bar_phase_at(target_output_frame)
```

For the pad, Rust computes:

```text
pad_frames_per_beat = output_sample_rate * 60 / pad_bpm
desired_sample_frame = phase_anchor_frame + target_bar_phase_beats * pad_frames_per_beat
```

The desired frame is then wrapped into the active loop region. If the configured loop region
is invalid or unavailable, the full sample region is used. If pad BPM is missing, invalid, or
the loop/sample region cannot produce a valid frame, playback falls back to the existing
loop-start behavior.

This model uses one bounded anchor and BPM metadata. It does not require full beat-grid
vectors or beat-detection work in the callback.

### Quantized Starts And Retriggers
When trigger quantization is disabled, `play_sample` and `play_sample_exclusive` retain the
current behavior: they start promptly at the effective loop start.

When trigger quantization is enabled with a selected grid step:

1. The audio thread computes the absolute output-frame target from the transport timeline.
2. The fixed-capacity scheduler stores the target and fixed-size command payload.
3. At execution time, Rust starts or restarts the pad at the phase-aligned sample frame for
   that target output frame when valid metadata is available.
4. If metadata is unavailable, Rust starts at the existing effective loop start.

The scheduled event target frame must be available to the execution path. The implementation
can pass the scheduler event's `target_frame` into command execution or store a bounded
precomputed phase descriptor in the scheduled command. Either approach must remain fixed-size.

### MultiLoop-Disabled Atomic Transitions
For MultiLoop disabled playback, the stop-all and phase-aware pad start remain one atomic
scheduled command at one output frame. If the scheduler rejects that command, currently
playing pads remain unchanged.

The target pad must still be validated before stopping active voices. A missing or invalid
target pad must not stop the current pad.

### BPM Lock Transport Phase Anchor
When BPM lock is enabled, Python already tracks the selected anchor pad and derives master
BPM from that pad's effective BPM and global speed. This change adds a fixed-size request
that lets the audio thread anchor transport phase from that selected pad when possible.

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
tempo-ratio matching without changing the transport downbeat anchor.

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
- Phase-aware starts can surprise users when a custom loop region does not begin on the pad
  anchor. Wrapping the desired frame into the active loop region preserves the chosen region
  while still aligning musical phase.
- BPM-lock anchoring from a currently playing pad is best-effort. If the anchor pad is not
  active, the engine should keep tempo matching rather than inventing phase.
- Active non-anchor pads may remain phase-offset until retriggered. Continuous correction is
  deliberately deferred to avoid a larger DSP change.

## Open Questions
- Whether a future UI should expose "phase-aware quantization" separately from trigger
  quantization, or whether quantized starts should always become phase-aware when metadata is
  valid.
- Whether pad phase should remain based on bar phase only for all current grid subdivisions.
- Whether an audio-to-control diagnostic is useful when BPM-lock phase anchoring cannot be
  established from the selected pad.
