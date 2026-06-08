## Context
Stage 5 unified accepted master-BPM updates across transport-grid timing and BPM-lock tempo
matching. The next architecture risk is not clock authority; it is ambiguity around which frame
space owns loop edits, prepared-stem alignment, and future per-stem/per-pad processing.

The current engine already separates these concerns in code:

- `TransportTimeline` and `TransportScheduler` operate in output frames.
- `RtMixer` voices store a per-voice `frame_pos` source-frame playhead.
- Python persists editable loop times in seconds and publishes them to Rust.
- Rust converts those times to frame positions at the mixer sample rate.
- Prepared stems are validated outside the callback, then rendered through the same voice
  playhead and loop path as full-mix playback.

This change makes that contract durable before any DSP foundation work starts.

## Goals
- Make source-frame position and output-frame time explicit architecture terms.
- Keep Rust authoritative for live source playheads and loop wrapping.
- Preserve Python as the owner of durable editable loop intent and project persistence.
- Make prepared-stem alignment requirements clear enough for future per-stem DSP buses.
- Identify click-producing state changes and sequence the later transition-smoothing work.

## Non-Goals
- Implementing a new crossfade, ramp, smoothing layer, or DSP graph.
- Changing waveform-editor UI behavior.
- Moving loop persistence out of Python.
- Replacing current EQ or adding new effects.
- Changing MIDI dispatch, trigger quantization, or explicit phase-anchor behavior.

## Proposed Design

### Frame Spaces
Use two names consistently:

- output frame: a rendered frame on the global Rust transport/scheduler timeline,
- source frame: a frame index in a pad's loaded full-mix buffer and aligned prepared stem buffers.

Output-frame scheduling decides when a trigger or stop becomes audible. Source-frame playback
decides which sample/stem frame is read once a voice is active.

Normal immediate and quantized triggers start at the effective loop start in source-frame space.
The existing explicit pad-derived phase-anchor path remains the only path that seeks based on
pad phase.

### Loop Model
Python continues to store editable loop settings in seconds because that is the durable UI and
project-file contract. The controller quantizes editable times to the cached output sample rate
where needed, then publishes bounded seconds through the existing Rust API. Rust converts the
published seconds to integer source frames using the mixer sample rate and stores a half-open
`[start, end)` loop region per pad.

The current immediate live-edit behavior remains:

- if an active voice's current source frame is inside the new loop region, the playhead is
  preserved;
- if it is outside, the voice is clamped to the new loop start on the next render;
- no crossfade or zero-crossing search is applied yet.

A future follow-up can replace the seconds-based control message with an explicit frame-range
message if float conversion becomes a real source of drift. That migration should be small and
OpenSpec-backed.

### Prepared Stems
Prepared stems are eligible only when they match the loaded full mix:

- same mixer output sample rate,
- same channel layout,
- same frame count,
- same source-version identity,
- same source-frame origin.

All-stems mode and enabled-stem mask changes select already accepted prepared buffers. They must
not restart voices, modify source-frame position, alter loop wrapping, or bypass BPM-lock/Key
Lock. The cached `instrumental.wav` artifact remains a cache artifact; it is not a fifth audible
component stem.

### Future Processing Attachment Points
The future DSP path should keep the source reader and loop wrap before per-stem or per-pad DSP:

```text
source/full-mix or aligned prepared stems
-> source-frame reader and loop wrap
-> playback-rate and Key Lock processing
-> optional future per-stem level/DSP
-> per-pad gain/current EQ or future per-pad DSP chain
-> deck/group/master buses
```

Per-stem processing must use bounded Rust-owned state and smoothed parameters. Python may persist
durable intent but must not become live audio truth for source-frame state.

### Click-Free Transition Plan
The current click risks are live loop edits, stem mode/mask changes, and immediate EQ/gain-like
parameter jumps. The follow-up plan is staged:

1. add a tiny Rust-side transition helper for short gain/crossfade ramps with preallocated state,
2. apply it first to stem mode/mask changes without changing visible controls,
3. then evaluate loop-boundary and loop-edit transitions separately,
4. only after that proceed to the DSP foundation and EQ replacement.

This change documents the plan and adds tests for the current alignment invariants. It does not
add the ramp helper.

## Risks And Trade-offs
- Seconds remain the Python persistence format, so float-to-frame conversion must stay covered by
  focused tests until a future explicit frame-range message exists.
- Immediate live loop edits preserve responsiveness but can still click.
- Prepared-stem masks are bounded and safe today, but future per-stem level/DSP needs smoothing
  before high-rate mapped controls are added.
