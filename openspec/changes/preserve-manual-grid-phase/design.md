## Context

The permanent Rust output-frame clock, bounded scheduler and timestamp/estimated-clock foundations
already exist. Adjust Loop derives its grid in the loaded source domain from the validated analysis
onset plus `pad_grid_offset_samples`; effective BPM is manual override first, analysis second.
Publication currently clips negative origins and narrows seconds to `f32`. Master-BPM changes
retain only modulo-four phase, which loses the beat count required for different loop lengths.

## Goals / Non-goals

Establish one signed source/master mapping, preserve full musical progression, define one-time
bootstrap and make pending-event policy explicit. Keep current immediate/future-grid loop-start
launch behavior active. Audible device/DSP preparation precedes subsequent synchronized-launch
activation; no hardware accuracy, latency compensation or runtime correlation is claimed here.

## Decisions

### Canonical signed source grid

Reuse the editor's anchor/BPM helpers and existing signed persistence. The analysis onset uses the
existing downbeat, beat, zero fallback and onset normalization. Round that onset in the editor's
loaded source sample-rate domain, add the stored integer offset, and publish its finite signed time
without clamping. Rust retains sufficient precision and represents the origin in its loaded-buffer
source-frame domain. Negative origins and origins outside the audio interval are musical references,
not read addresses; no source buffer access uses an unchecked origin.

For valid source BPM and source rate, `frames_per_beat = rate * 60 / bpm` and source beat at frame
`s` is `(s - signed_origin) / frames_per_beat`. Euclidean modulo yields stable beat/bar phase even
before the origin. Loop-start phase derives from this same expression, not a second zero anchor.

Grid origins and native loop-region seconds use `f64`, including direct-MIDI runtime loop metadata,
until source-frame conversion. This preserves integer editor markers at long source positions and
corrects narrowing under the existing `loop-region` sample-accurate marker requirement; it does not
change the persisted format or loop-edit contract. Pad/master BPM retain their established native
`f32` parameter precision and are promoted to `f64` for phase arithmetic. The mapping uses those
accepted native BPM values, without asserting exact representation of arbitrary decimal tempos.

### Source/master mapping and supported loops

The shared Rust helper converts master musical position to a loop-relative beat offset from the
canonical loop-start beat, wraps by the loop's exact musical tick length, then converts to source
frames within the effective half-open loop. Wrapping before frame conversion avoids accumulated
cycle drift from integer-rounded loop duration at fractional BPM.
Mapping reads metadata only and leaves loop markers, voice positions and source/stem ownership
unchanged. Full mix and stems use one eventual mapped source address.

A loop is musically compatible when its positive 1/64-note tick count either divides 64 (short
repetitions that evenly tile a bar) or is a multiple of 64 (whole bars), and the physical length
differs from that ideal duration by at most one source frame. This tolerance accepts integer frame
rounding, not off-grid musical durations. Differing whole-bar lengths retain beat/bar phase. Short
loops retain their repeated loop-cycle phase within each master bar; their source bar labels do not
equal the complete master bar position on every repeat. Missing/invalid metadata returns unavailable
mapping. Unsupported valid physical loop lengths retain physical wrapping as an internal fallback,
with compatibility false and no sustained synchronization claim. Current ordinary playback remains
available. Tempo ratios clipped outside the engine's `0.5..2.0` range also receive no sustained-sync
guarantee.

### Musical-position transport anchor

Store musical position at an output-frame reference and derive arbitrary target positions by
output-frame distance and current master frames per beat. Preserve the complete beat value at the
current frame on accepted BPM changes. Modulo-four calculations are derived views of that position.
This retains loop-cycle progression through tempo changes without resetting frame time or moving
active source anchors. Any downbeat frame exposed to existing diagnostics is derived consistently.

### One-time selected-reference bootstrap

`bootstrap_transport_from_pad(id)` is a dedicated bounded control request emitted by the shared
BPMLOCK selected-reference setup used on enable and restore. Rust latches the first accepted
reference per stream. When that pad is active with valid signed grid/BPM metadata, transport
musical position is anchored to its absolute source beat at the current output frame. If the pad
or metadata is unavailable, bootstrap remains pending and retries at successful start or relevant
metadata publication after the bounded callback message/parameter batch; it does not choose another
playing/oldest pad.

Further reference requests cannot replace a latched reference. Unloading an incomplete reference
clears that pending reference, leaving bootstrap unused; a later explicit reference request can
select another pad. Successful bootstrap consumes the opportunity permanently for that stream.
Silence, stop/restart, reference changes, wraps and stem masks cannot rearm it. Explicit
`anchor_transport_phase_from_pad(id)` remains a deliberate repeatable operation; success also
consumes bootstrap. A fresh stream has a fresh unused opportunity. Neither runtime latch nor
transport phase is persisted. Bootstrap changes transport phase without resetting the output-frame
clock or seeking/retriggering existing voices.

Accepted loop edits invalidate a voice's old source/output timeline anchor. Bootstrap phase reads
and rendering share bounded `playhead_before_render` normalization: normal positions outside the
effective loop clamp to its start, while explicit seek behavior remains consistent. Bootstrap
therefore uses the same first source frame the renderer will read rather than an obsolete pre-edit
position. DSP state construction or persisted marker changes are not part of this normalization.

### Pending-event policy during this foundation

Once scheduled, an event keeps its absolute output target, stable sequence and original optional
input timestamp. Master BPM/grid/bootstrap or pad metadata changes do not reround, cancel or
reschedule it. New requests use the updated master grid. At execution, existing loop-start launch
semantics read the latest accepted effective loop/BPM metadata. An accepted loop edit can therefore
change the loop start used by a pending launch without changing its output target. Existing
stop/unload and missing-source safety behavior remains unchanged. This slice does not introduce
pending-start cancellation; the synchronized-launch activation stage must define that policy.

This freezes the current scheduler contract only. Captured-input musical targets across historical
clock snapshots, nearest targets, stale timestamps and late source catch-up still need the explicit
subsequent activation delta. Do not infer that a timestamp field already enables that policy.

## Risks / Trade-offs

- Signed/fractional frame references must never be cast to unsigned read positions before wrapping.
- Musical compatibility is deliberately narrower than valid playback; unsupported loops are
  reported as such rather than presented as synchronized.
- Bootstrap may complete after metadata arrives, but it must retain the selected reference and
  preserve active source playheads. Automatic post-silence reselection would violate this policy.
- Two bounded queue classes apply at callback boundaries; bootstrap retries use the latest
  accepted audio-thread state and must not infer Python enqueue timing as live truth.
- Preserving full beats through BPM changes establishes source math, not corrected audible DSP
  delay. Rubber Band/device measurements remain necessary before launch activation.

## Validation

Test positive/negative origins, near-zero and fractional BPM, 44.1/48 kHz source/output domains,
manual overrides/clear and restore. Cover loop-start phase, half-open wrapping, unequal whole-bar
lengths, evenly tiling short loops, exact musical-period wrapping, one-frame tolerance and unavailable
metadata. Test bootstrap request latching, delayed
availability, missing/unloaded reference, no rearm after silence and deliberate sync. Test complete
beat continuity through BPM changes and stable accepted target/sequence/stamp across metadata edits.
Run official strict OpenSpec validation plus full project checks and maintained-doc impact review.
Keep generated audio/logs in local workspace scratch/exports. No commit or push follows automatically.
