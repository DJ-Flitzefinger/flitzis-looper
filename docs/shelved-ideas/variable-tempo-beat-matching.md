# Variable-tempo beat matching

The former SYNC / REALTIME BPM experiment aimed to keep PADs with changing
recorded tempo in time with BPM Lock. Examples include live performances with
tempo variation and vinyl transfers with gradual speed drift. The first useful
target is a PAD following Looper's stable master tempo. A beat map can align
pulses at its resolution. Correcting pitch or speed fluctuations within a beat,
following live audio, and letting a variable-tempo recording drive the master
are separate problems.

## What the previous attempt established

The October 27 listening report found that a 64-bar/8-bar pair stayed aligned
with SYNC off and sounded worse with it on. The software reproduction refused
live adoption under either of the two possible BPM Lock references. The
reference used in the listening session was unknown. A separate prepared
restart entered mapped playback, but the difference between detector-beat
positions grew from about 0.008 to 0.146 quarter notes. The target difference
came from the two maps at the user grid origins, so that metric could not
establish musical correctness. Live adoption and musical phase each needed
their own proof. The later constant-BPM listening report covered over 100 bars
without audible drift on the tested pair. The nearby Tap estimate produced
slight but acceptable drift. [Live toggle diagnostic](../../openspec/changes/archive/2026-10-30-adopt-live-source-sync-policy/results.md#fresh-scalar-to-sync-diagnostic-2026-10-27),
[constant-tempo listening receipt](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/dbc80d3c117dcec50996feef2e8c59f1649388a5/docs/sync-archive.md).

Rendering had a separate failure. The cold Native/FIFO comparison met all
original acoustic bounds in 0/64 cases. Offline Rubber Band R3 met them in
2/16 cases in its tested Key Lock ON, phase-zero domain. A later test retained
72 complete streams that agreed with the reference C-ABI stream and source
coordinates across caller partitions, yet 0/60 isolated responses met both
attack timing predicates. Some responses changed shape enough that a single
delay could not align them. In one 48 kHz impulse fixture, preserving the
response required 1,598 frames (33.292 ms) before the target. Strict first
sound at the target cut into that response even with preparation. A correct
internal read, a successful map adoption, and audible timing are distinct results. [Renderer comparison](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/949cff1dae4b979639dca1cbc1e86c586387cd21/docs/accepted-map-render-comparison.md),
[owning-future measurement](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/949cff1dae4b979639dca1cbc1e86c586387cd21/docs/beatmap-sync-design.md#L815-L824),
[causal headroom fixture](../../openspec/changes/archive/2026-10-30-add-key-transposition-performance/evidence/key-lock-backend.md).

## What to reuse from Mixxx

Mixxx 2.5's **Sync Lock** uses a beat grid analyzed ahead of playback. A
dynamic leader can preserve its recorded tempo changes, or the DJ can choose
a steady tempo. The follower derives a nominal rate from the reference and
local tempo, then applies bounded beat-phase feedback for eligible Quantize
playback. The local tempo estimate spans neighboring beats. The useful
principles are an explicit reference and independent treatment of tempo
and phase. Phase modulo one beat leaves bar position and beat-count mistakes
unresolved. [Mixxx Sync Lock manual](https://manual.mixxx.org/2.5/en/chapters/djing_with_mixxx#sync-lock-with-dynamic-tempo),
[local tempo lookup](https://github.com/mixxxdj/mixxx/blob/2.5.0/src/engine/controls/bpmcontrol.cpp#L1118-L1144),
[follower rate](https://github.com/mixxxdj/mixxx/blob/2.5.0/src/engine/sync/synccontrol.cpp#L229-L275),
[phase feedback](https://github.com/mixxxdj/mixxx/blob/2.5.0/src/engine/controls/bpmcontrol.cpp#L432-L545).

For a Looper PAD following a fixed master with a known map, explicit source
anchors are a more direct starting point. Ableton's Warp Markers illustrate
the mapping of positions in a recording to musical time. [Ableton Warp Markers](https://www.ableton.com/en/live-manual/12/audio-clips-tempo-and-warping/#warp-markers).

## Fit to the current engine

- **Keep the existing output clock and launch policy.** Rust's
  `TransportTimeline` projects master beats from rendered output frames, and the
  scheduler applies captured GUI/MIDI intent at the due frame. Quantize selects
  the launch frame. Continuous mapped playback would start from a separately
  chosen source beat. The loop entry, Grid Offset, and downbeat are different
  choices. The mapped path reads the master slope once. Ordinary scalar BPM
  Lock currently uses a source-period/master-period ratio. Applying that ratio
  again to a mapped trajectory would double-count tempo. [Runtime and transport](../architecture/runtime-and-transport.md),
  [grid and source timing](../architecture/grid-and-source-timing.md).
- **Derive playback from source positions.** Use a monotone sequence of
  source-time and beat-coordinate anchors with explicit interval counts and
  coverage. For a launch at output frame `n₀` and source beat `b₀`, look up the
  source position corresponding to `b₀ + M(n) − M(n₀)`, where `M` is the
  existing master beat projection. Share the forward and inverse interpolation
  across preparation, rendering and any editor. Fold loop repeats over their
  qualified musical span, then translate into legal reads within the physical
  PCM window. Keep source zero independent of the resident view and preserve
  decoder/resampling provenance. Routing instantaneous BPM through the scalar
  rate glide changes the integrated phase. Periodic seeks cut reader and DSP
  history. [Grid and source timing](../architecture/grid-and-source-timing.md),
  [media and PCM](../architecture/media-and-pcm.md).
- **Keep one read and publication path.** Extend the existing source trajectory
  and `SourceReadPlan` boundary as needed. Today it describes physical and
  accepted scalar loop domains, and the dry, pitch/history and stem paths
  already share source addressing. Build and validate maps off the callback.
  Bind the immutable map to original/decoder identity and recheck content
  generation, map and loop revision, resident coverage, and the applicable
  master epoch at adoption. Refuse maps with missing coverage or required rates
  outside the supported range. Each voice keeps independent mutable state. Reuse
  existing due-frame admission and reader retirement. A stale or unsupported
  effect should preserve the current audio and report its outcome. A matching
  acknowledgement proves adoption for that source, while a render observation
  establishes what was read. [Architecture](../architecture.md),
  [media and PCM](../architecture/media-and-pcm.md).

## A small experiment for a later revisit

1. Use hand-checked anchors for a genuinely drifting recording and an independent
   set of beat annotations. Compare ordinary scalar BPM Lock with a fixed-master,
   full-mix dry render from the map, using a prepared launch from stopped, zero
   transpose and Key Lock off. Dry playback changes pitch with speed. Check long-run beat phase,
   first attack, loop seams and callback-partition invariance. A constant-tempo
   case controls for regressions. Require a difference one can hear or measure
   against the independent annotations before broadening the design.
2. If that works, evaluate automatic maps from off-thread full-source analysis.
   Keep raw detector hits separate from interpreted beat counts, uncertain or
   missing spans, and known downbeats. Use the same independent scoring and
   listening comparison. The current Beat This path produces a scalar estimate.
   Its source custody and worker boundary are reusable. The old regional-probe
   WIP compared fractional and integer scalar BPM on common, independently
   supported windows. It was reviewed and typechecked, with installation and
   listening acceptance still open. [BPM analysis](../architecture/bpm-analysis.md).
3. Qualify later capabilities individually. Pitch-preserving rendering must
   show that intended beats and actual output attacks agree after DSP. Offline
   warping is useful as a comparison and trades callback work for preparation,
   storage and invalidation after map or tempo changes. Live switching, pause
   and Hold, seeks, loop edits, and shared timing across prepared stems each
   need explicit behavior and their own evidence. The former branch's renderer,
   live adoption machinery and control bindings are historical evidence.
   [Pitch and Key Lock](../architecture/pitch-and-key-lock.md),
   [stems](../architecture/stems.md).

The common ancestor `949cff1d` on `main` contains the old SYNC implementation
and original reports. [Historical design](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/949cff1dae4b979639dca1cbc1e86c586387cd21/docs/beatmap-sync-design.md),
[historical feasibility research](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/949cff1dae4b979639dca1cbc1e86c586387cd21/docs/beatmap-sync-research.md).
