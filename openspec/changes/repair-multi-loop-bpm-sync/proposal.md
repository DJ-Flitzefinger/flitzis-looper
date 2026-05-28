# Change: Repair Multi Loop BPM-Lock synchronization

## Why

The Rubber Band Key Lock branch exposes a timing defect that is broader than the Key Lock backend:
with Multi Loop and BPM Lock enabled, two pads that should share one musical loop cycle can drift
apart while normal loop wrapping continues. The drift is especially visible around `1.5x` global
Pitch/Speed with variable render segment sizes, and it keeps accumulating across repeated loop
wraps. Manual START/STOP or retrigger resets the audible phase because the voices restart from
their effective loop starts.

The current Rust mixer advances each voice from a local source-frame playhead by rounding
`rendered_output_frames * tempo_ratio` for each render chunk. The Rust `TransportTimeline` already
owns the permanent output-frame clock for trigger quantization and master-BPM phase, but active
voice loop wrapping does not yet derive its BPM-locked phase from that shared clock.

The Loop Editor Grid Offset path is also part of the same timing foundation. Its anchor and
snapped loop markers are source-domain facts and must not move when playback speed, BPM Lock, Key
Lock, trigger quantization, master BPM, or other pad playback state changes.

## What Changes

- Define the BPM-locked Multi Loop phase-stability contract across repeated loop-region wraps.
- Require the repair to cover speed ratios `1.0`, `1.25`, `1.5`, and `2.0`, Key Lock off/on,
  fixed and variable callback segment sizes, same-BPM controls, and different-BPM pads matched to
  one master tempo.
- Preserve manual START/STOP and retrigger as phase resets from the effective loop start.
- Require full-mix and prepared-stem playback to share the same BPM-locked timing path.
- Keep missing BPM metadata on the documented global-speed fallback without claiming phase lock.
- Keep Loop Editor Grid Offset, snapped loop markers, and visible source-side grid anchors stable
  under playback sync changes.
- Preserve the realtime callback boundary: no disk I/O, JSON, Python/GIL access, UI work,
  blocking locks, logging, neural inference, plugin loading, unbounded loops, heavy allocation, or
  long-running work.

## Impact

- Affected specs: `multi-loop-mode`, `time-stretch-pitch-shift`, `waveform-editor`.
- Affected code: Rust mixer voice timing, audio-stream segment boundary plumbing if needed,
  prepared-stem/full-mix timing tests, and Python Loop Editor grid stability tests.
- Affected docs: `docs/architecture.md` only if the implementation changes the documented
  ownership model.

## Non-Goals

- No new plugin host or external FX backend.
- No realtime stem separation or neural inference.
- No change to Python ownership of durable loop intent, project persistence, or Grid Offset edit UX.
- No live loop-edit crossfade or click-removal policy.
- No automatic phase forcing for non-BPM-locked pads or pads without valid BPM metadata.
- No change to quantized trigger semantics that would start newly triggered pads in the middle of
  their source loop.
