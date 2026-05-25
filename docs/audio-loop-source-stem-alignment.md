# Audio Loop Source And Stem Alignment

Date: 2026-05-25

Status: Stage 6 architecture clarification. This document does not implement EQ, DSP effects,
plugin hosting, a new smoothing layer, or runtime stem separation.

## Purpose

This document makes the loop/source-position/stem model explicit before DSP/FX foundation work.
It complements `docs/audio-performance-architecture-audit.md` and
`docs/audio-state-ownership.md`.

## Terms

- Output frame: a rendered frame on the Rust transport/scheduler timeline.
- Source frame: a frame index in a pad's loaded full-mix buffer and any aligned prepared stems.
- Loop region: a half-open source-frame range `[start, end)` used by the Rust mixer.
- Voice playhead: the active Rust voice's current source-frame position.

Output-frame time decides when a trigger or stop becomes audible. Source-frame position decides
which sample or stem frame is read after a voice is active.

## Current Model

Python persists editable loop intent in seconds, plus auto-loop flags, bar count, and grid offset.
The controller quantizes loop times to the cached output sample rate where needed and publishes
bounded seconds to Rust. Rust converts those seconds to integer source frames at the mixer sample
rate and stores the live loop region in `RtMixer`.

Normal immediate and quantized triggers start from the effective source-frame loop start.
Quantization selects the output-frame start time only. It does not seek into the source loop or
make an active pad the master clock. The explicit `anchor_transport_phase_from_pad(...)` path
remains the separate pad-derived phase-sync operation.

Live loop edits are immediate. On the next render:

- if the current voice playhead is inside the new loop region, it is preserved;
- if it is outside the new loop region, it is clamped to the new loop start;
- no crossfade, zero-crossing search, or click suppression is applied yet.

Prepared stems are accepted only after offline/control-plane validation and Rust publication. A
prepared set must match the loaded full mix by source version, sample rate, channel layout, frame
count, and source-frame origin. Once accepted, all-stems rendering uses the same voice playhead,
loop wrapping, BPM-lock ratio, Key Lock processing, gain/EQ, metering, and playhead telemetry as
full-mix playback.

The cached `instrumental.wav` artifact is cache data, not a fifth component layer. The performance
`I` preset means Drums + Melody + Bass.

## Decisions

- Rust owns live source playheads, loop wrapping, prepared-stem handles, stem mode/mask state, and
  future transition/DSP state.
- Python owns durable loop-edit intent, persistence, waveform editor UX, stem cache metadata, and
  offline/background generation orchestration.
- Output-frame scheduling and source-frame playback must stay separate in code and specs.
- Future per-stem processing should attach after the source-frame reader and loop wrap, before the
  per-pad bus or future per-pad DSP chain.
- Future high-rate or click-sensitive controls need Rust-side smoothing or crossfade state before
  they are treated as professional DSP controls.

## Follow-Up Plan

The next click-free preparation should be a bounded Rust transition helper, not a visible effect:

1. Define a tiny preallocated gain/crossfade ramp state for already selected buffers.
2. Apply it first to stem mode/mask transitions, because they are bounded source selections and do
   not require loop-boundary detection.
3. Evaluate live loop-edit transition policy separately, because moving loop start/end can require
   different treatment than selecting stems.
4. Only after these transition semantics are clear should the DSP foundation and EQ replacement
   proceed.

Non-goals remain unchanged: no new EQ, no new DSP/FX, no plugin hosting, no real-time stem
separation, no disk I/O, no Python/GIL access, no logging, no blocking waits, and no heavy
allocation in the audio callback.
