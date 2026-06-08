# Change: Multi Loop BPM-Lock synchronization

## Why

Multi Loop and BPM Lock require one coherent timing model across Rust transport
time, active voice source-frame addressing, prepared stems, Key Lock, trigger
quantization, and Loop Editor grid metadata.

The Rust `TransportTimeline` owns the absolute output-frame clock for master
BPM, downbeat anchoring, and trigger quantization. BPM-locked active voices with
valid master and pad BPM metadata use that timeline through fixed
output-frame/source-frame anchors so pads representing the same musical loop
length share loop boundaries across repeated wraps.

The Loop Editor Grid Offset path is part of the same timing foundation. Its
anchor and snapped loop markers are source-domain facts and stay stable when
playback speed, BPM Lock, Key Lock, trigger quantization, master BPM, or other
pad playback state changes.

## What Changes

- Define the BPM-locked Multi Loop phase-stability contract across repeated
  loop-region wraps.
- Cover speed ratios `1.0`, `1.25`, `1.5`, and `2.0`, Key Lock off/on, fixed
  and variable callback segment sizes, same-BPM controls, and different-BPM pads
  matched to one master tempo.
- Preserve manual START/STOP and retrigger as phase resets from the effective
  loop start.
- Require full-mix and prepared-stem playback to share the same BPM-locked
  timing path.
- Keep missing BPM metadata on the documented global-speed fallback without
  claiming phase lock.
- Keep Loop Editor Grid Offset, snapped loop markers, and visible source-side
  grid anchors stable under playback sync changes.
- Preserve the realtime callback boundary: no disk I/O, JSON, Python/GIL
  access, UI work, blocking locks, logging, neural inference, plugin loading,
  unbounded loops, heavy allocation, or long-running work.

## Impact

- Affected specs: `multi-loop-mode`, `time-stretch-pitch-shift`,
  `waveform-editor`.
- Affected code: Rust mixer output-frame anchored voice timing, audio-stream
  segment boundary plumbing, prepared-stem/full-mix timing tests, and Python
  Loop Editor grid stability tests.
- Affected docs: architecture, Key Lock backend, Rust module, and project
  overview documentation describe the current timing model.

## Non-Goals

- No new plugin host or external FX backend.
- No realtime stem separation or neural inference.
- No change to Python ownership of durable loop intent, project persistence, or
  Grid Offset edit UX.
- No live loop-edit crossfade or click-removal policy.
- No automatic phase forcing for non-BPM-locked pads or pads without valid BPM
  metadata.
- No change to quantized trigger semantics that would start newly triggered pads
  in the middle of their source loop.
