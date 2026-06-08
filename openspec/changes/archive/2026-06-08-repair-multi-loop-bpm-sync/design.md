# Design: Multi Loop BPM-Lock timing model

## Overview

Gen3 keeps output-frame transport time and source-frame editing state as
separate domains:

- `TransportTimeline` owns the absolute output-frame clock, master BPM, and
  downbeat anchor.
- `TransportScheduler` uses output-frame targets for immediate and quantized
  trigger execution.
- `RtMixer` owns active voice source-frame playheads, loop regions, source
  selection, tempo-ratio application, Key Lock, Gain/Trim, DSP, and metering.
- Python persists loop/grid intent in seconds and publishes bounded loop/timing
  metadata to Rust.
- Prepared stems are source-aligned buffers and use the same source-frame
  addressing path as the full mix.

BPM-locked active voices with valid master and pad BPM metadata derive loop
phase from the Rust output-frame timeline through fixed
output-frame/source-frame anchors. The Loop Editor grid, Grid Offset, loop
markers, source buffers, and prepared stems remain source-frame facts.

## Time Domains

### Source-Domain Facts

- Loaded full-mix source frames.
- Prepared stem source frames and source-version alignment.
- Python loop start/end intent and Rust half-open source-frame loop regions.
- Analysis onset/downbeat and per-pad Grid Offset.
- Loop Editor visible musical grid and snapped loop marker positions.
- Rust pad timing metadata published from the Loop Editor source anchor.
- Voice source-frame read position and playhead telemetry.

These values stay stable when global playback modes change unless the performer
edits pad loop/grid settings or loads/replaces the pad's source audio.

### Output-Domain Facts

- `TransportTimeline.output_frame`.
- Transport master BPM and downbeat anchor.
- Trigger quantization target frames.
- Callback and scheduled segment start/end output-frame bounds.
- Shared BPM-locked musical phase for active voices with valid BPM metadata.

These values decide when audio is rendered and how a BPM-locked voice maps the
shared output phase to a source-frame position.

## Render Segment Timing

`audio_stream.rs::render_mixer_segment(...)` passes each scheduled segment's
absolute output-frame start into `RtMixer::render_rt_at_output_frame(...)`.
`RtMixer` renders that segment with the same callback-owned realtime state used
by immediate paths.

`VoiceSlot` stores an optional `PlaybackTimelineAnchor`:

```text
output_frame: absolute output frame for the voice timeline anchor
source_frame: source frame corresponding to that output frame
```

Start, retrigger, seek, pause/resume, and loop-boundary state changes establish
or refresh the anchor from the current segment output frame and voice source
frame. During BPM-locked playback with valid master and pad BPM metadata,
`RtMixer` computes:

```text
source_offset = round((segment_output_frame - anchor.output_frame) * tempo_ratio)
```

The resulting source offset is mapped through the voice's half-open source-frame
loop region. The same mapping also determines the next source frame recorded for
playhead telemetry. Pads that represent the same musical loop length therefore
share output-frame loop boundaries across repeated wraps, including fixed and
variable callback segment sizes.

Pads without valid BPM metadata use the global speed multiplier. They can play
concurrently in Multi Loop mode, but they do not define the BPM-locked phase
path used by pads with valid metadata.

## Loop Editor And Trigger Quantization

The Loop Editor grid is a source-domain editing grid. Python derives the grid
anchor from pad analysis metadata plus `pad_grid_offset_samples`, renders
visible grid lines from that anchor, stores snapped loop markers in source time,
and publishes the same anchor to Rust as pad timing metadata.

Trigger quantization uses the Rust transport grid only to choose an output-frame
start time. It does not seek inside the source loop and does not move the Loop
Editor grid or snapped source markers.

## Stems, Key Lock, And DSP

Full-mix and prepared-stem playback use the same source-frame address for a
given voice frame before Gain/Trim, DSP, metering, and playhead telemetry.

Key Lock consumes the same source-frame sequence as varispeed playback. Rubber
Band LiveShifter state, fixed staging buffers, and bounded FIFOs belong to the
voice slot and do not redefine source loop ownership, trigger quantization, or
the Rust master output timeline.

## Realtime Constraints

The audio callback must not perform disk I/O, JSON reads/writes, Python/GIL
work, UI work, blocking locks, logging, neural inference, plugin
loading/scanning, unbounded loops, heavy allocation, or long-running work.

Voice sync metadata is fixed-size state stored in existing voice/mixer
structures. Rendering does not allocate per sample, per loop wrap, or per
callback chunk.

## Validation Strategy

- Rust tests cover first-pass and repeated-wrap phase stability, retrigger reset,
  fixed and variable segment sizes, Key Lock off/on, same-BPM controls,
  different-BPM matched pads, missing BPM metadata, and prepared-stem parity.
- Python controller tests cover Loop Editor Grid Offset and snapped-loop
  stability under speed, BPM Lock, Key Lock, trigger quantization, master-BPM
  recomputation, and other-pad playback.
- OpenSpec strict validation covers the active behavior requirements.
- The full validation path uses uv-managed Rust and Python commands from the
  repository root.
