# Design: Multi Loop BPM-Lock synchronization repair

## Context

Gen3 already separates output-frame transport time from source-frame playback:

- `TransportTimeline` owns the absolute output-frame clock and master BPM.
- `TransportScheduler` uses output-frame targets for trigger quantization.
- `RtMixer` stores active voice source-frame playheads and loop regions.
- Python persists loop/grid intent in seconds and publishes bounded loop/timing metadata to Rust.
- Prepared stems are aligned source buffers and use the same voice playhead path as the full mix.

The current defect sits at the boundary between the first two bullets and the active voice path.
Normal loop wrapping advances each voice locally by an integer number of source frames per render
chunk. Different BPM ratios can round differently, so pads that should complete one shared musical
cycle together accumulate independent phase error.

## Goals

- Use the Rust master output timeline as the authority for shared BPM-locked musical phase.
- Keep the Loop Editor grid, Grid Offset, loop markers, source buffers, and prepared stems in
  source-frame space.
- Prevent BPM-locked active voices from accumulating per-callback rounding drift across repeated
  loop wraps.
- Preserve existing immediate trigger, quantized trigger, stop, pause/resume, seek, retrigger,
  loop edit, Key Lock, Gain/EQ, metering, and prepared-stem behavior unless the spec explicitly
  says otherwise.
- Keep all callback work bounded and realtime-safe.

## Non-Goals

- No plugin hosting, external FX graph, or new DSP module.
- No realtime stem generation.
- No live loop-edit crossfade or zero-crossing search.
- No source-grid reinterpretation as a global transport grid.
- No automatic sync for pads without valid BPM metadata.

## Time Domains

### Source-domain facts

- Loaded full-mix source frames.
- Prepared stem source frames and source-version alignment.
- Python loop start/end intent and Rust half-open source-frame loop region.
- Analysis onset/downbeat and per-pad Grid Offset.
- Loop Editor visible musical grid and snapped loop marker positions.
- Rust pad timing metadata published from the Loop Editor source anchor.
- Voice source-frame read position and playhead telemetry.

These values must remain stable when global playback modes change unless the performer edits pad
loop/grid settings or loads/replaces the pad's source audio.

### Output-domain facts

- `TransportTimeline.output_frame`.
- Transport master BPM and downbeat anchor.
- Trigger quantization target frames.
- Callback/segment start and end output-frame bounds.
- Shared BPM-locked musical phase for active voices that have valid BPM metadata.

These values decide when audio is rendered and how a BPM-locked voice maps the shared output phase
to a source-frame position.

## Candidate Repair Direction

The preferred direction is output-frame anchored source addressing:

1. Thread the absolute segment output-frame range from `audio_stream.rs::render_mixer_segment(...)`
   into the mixer.
2. Store minimal per-voice sync metadata at start/retrigger time, such as the output frame where
   the voice became active, its effective loop start, loop length, BPM, and the source frame that
   corresponds to the start output frame.
3. For BPM-locked voices with valid metadata, derive the loop-relative source frame for each
   rendered output frame from absolute output-frame delta and the active tempo ratio, using a
   non-cumulative calculation.
4. Keep local source-frame advancement as the fallback for non-BPM-locked voices, missing metadata,
   explicit before-loop/after-loop seek modes where appropriate, or implementation cases that have
   not yet been made output-frame-addressable.
5. Keep Rubber Band, Gain/EQ, stem selection, and metering attached after the source reader so
   they consume the same corrected source-frame sequence.

A fractional source-position accumulator is a narrower fallback if it can prove no drift across
all required cases, but it is weaker because it still leaves each pad as an independent clock. A
hard master-loop retrigger at boundaries is also weaker because it risks audible discontinuities
and Rubber Band state resets. Any selected approach must be justified against the tests in this
change before production code lands.

## Selected Strategy

Use output-frame anchored source addressing for BPM-locked active voices with valid pad BPM
metadata.

Rejected options:

- Fractional source-position accumulator: it reduces callback-rounding error, but each pad still
  owns an independent local clock and can diverge through loop edits, segment splits, or future
  timing paths.
- Master-loop boundary retrigger/snap: it can hide accumulated error at boundaries, but it risks
  audible discontinuities, Rubber Band FIFO/state resets, and click behavior that this change does
  not specify.
- Standalone hard phase correction: it has similar click and state-reset risks unless combined
  with a deeper source-addressing repair.

The implementation should thread absolute segment output-frame bounds into the mixer and attach
fixed-size sync metadata to voices at start/retrigger time. For BPM-locked voices with valid
metadata, source frames should be derived from absolute output-frame position plus voice sync
metadata, not from cumulative per-callback rounded source-frame increments. Non-BPM-locked voices,
voices missing valid BPM metadata, and explicit seek edge modes may initially keep the existing
local progression path where the documented behavior requires it.

## Realtime Constraints

The audio callback must not perform disk I/O, JSON reads/writes, Python/GIL work, UI work,
blocking locks, logging, neural inference, plugin loading/scanning, unbounded loops, heavy
allocation, or long-running work.

Any additional voice sync metadata must be fixed-size state stored in existing voice/mixer
structures before or during bounded callback control paths. The repair must not allocate per
sample, per loop wrap, or per callback chunk.

## Validation Strategy

- Keep the ignored Rust reproduction until the repair is ready, then unignore it or replace it
  with equivalent passing regression tests.
- Add focused Rust tests for first-pass drift, repeated wraps, retrigger reset, fixed and variable
  segment sizes, Key Lock off/on, same-BPM controls, different-BPM matched pads, missing BPM
  fallback, and prepared-stem parity.
- Keep Python controller tests for Loop Editor Grid Offset and snapped loop stability under speed,
  BPM Lock, Key Lock, trigger quantization, master-BPM recomputation, and other-pad playback.
- Run strict OpenSpec validation for this change.
- Run uv-managed Rust checks/tests and focused Python tests before considering the repair complete.
