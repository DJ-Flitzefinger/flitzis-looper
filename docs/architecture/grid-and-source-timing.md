# Grid and source timing

The source grid assigns a continuous quarter-note coordinate to loaded source
positions. The output timeline schedules effects in rendered output frames.
[Runtime and transport](runtime-and-transport.md) owns that clock.
[BPM analysis](bpm-analysis.md) supplies a constant-period estimate.

## Address spaces and projection

| Domain | Used by |
| --- | --- |
| Original decoded frames and rate | Complete decoder artifact and resampling. |
| Absolute loaded frames and rate | PCM, waveform, loop markers and source reads. A resident view translates absolute frames to its local allocation. |
| Binary64 seconds from source zero | Saved intent, waveform X and bounds, seek, duration and playhead APIs. |
| Continuous source beats | Source grid, snapping, loop length and phase; may use a signed origin. |
| Absolute output frames | Native clock and scheduled events, including during silence. |

[`SourceGrid`](../../rust/crates/looper/src/audio_engine/source_grid.rs) projects
`source = origin + beat_index * period` and its inverse. Its control-only
[`ScalarSourceGrid`](../../rust/crates/looper/src/audio_engine/scalar_grid.rs)
binding supplies the [waveform grid](../../src/flitzis_looper/ui/waveform_grid.py),
snap and auto-loop calculations. The grid base comes from applicable timing
or the supported legacy/activity anchor. Grid Offset applies a signed,
whole-loaded-frame displacement independently of the loop start. The finest
musical snap is 1/16 beat. Compute each absolute boundary, then round once
(nearest-even in the editor) and clamp to the source. Auto-loop adds
`4 * bars` beats at the chosen start before rounding its endpoints.
Loop-relative labels change the numbering while retaining the source positions.

Raw waveform X derives from the absolute loaded frame divided by loaded rate.
Bounds recover exact frame edges passed as `frame/rate` with a tightly bounded
tolerance. Other fractional bounds use floor-start/ceil-end coverage. Amplitudes
are float32 and visible plotting arrays can promote Y to match float64 X.
Seek addresses a physical frame using the Normal/Intro/Tail read policy.
Playhead reports the floored next-source cursor as binary64 seconds.
Finite views preserve source zero and extent through zoom, seek and preparation.

## Period, origin and drift

A period error accumulates with beat count. An origin error shifts the entire
grid by a constant amount. The legacy QM helper
[`calculate_bpm`](../../rust/crates/analysis/src/lib.rs) averages adjacent
intervals. On ordered detector positions this reduces to
`(last - first) / (count - 1)`, so endpoint lattice error and wrong counts
can bias the slope even with an accurate origin and a displayed `120.00`.
Current automatic analysis fits supported regions and checks alternate period
hypotheses. More display digits, a phase edit and a different rounding rule
cannot establish correct period or musical count.
The [original drift investigation](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/1590e4c3e85bfbbafd63521f083068e1631214c1/docs/grid-timing-diagnosis.md)
records the old estimator and coordinate measurements.

Manual/Tap intent takes precedence over the current acknowledged accepted
Automatic period or ordinary Legacy BPM. Accepted results use their exact
binary64 period and signed origin. Scalar BPM converts via `60 / BPM`.
Untouched or reverted field edits publish no override. Approximate scalar-only
metadata carries its warning and provenance without supplying a phase or
accepted period. Manual BPM and independent Grid Offset remain available.

## Accepted Automatic timing

The explicit route uses
[`accepted_publication.py`](../../src/flitzis_looper/controller/accepted_publication.py)
and the [Native API](../../src/flitzis_looper_audio/__init__.pyi).
`prepare_constant_timing` captures the current source and request and produces
complete PCM/QM evidence in an opaque ticket. `publish_constant_timing` checks
that evidence against explicit count hypotheses, error and acceptance provenance,
and an independently declared signed origin. Derived fit intercepts, nominal
BPM and raw detector data do not supply that approval.

`current_constant_timing` requires matching content/source identity, generation,
rate, Automatic intent and callback acceptance. Ticket status distinguishes
capture, pending, accepted and rejected. Ticket metadata can describe an older
adoption after it stops being current. Manual/Tap/Legacy edits or replacement
retire older authority. A temporary unreadable placement can defer this operation
using its original ticket. Loop and master refresh has its own acknowledgement.
GUI and Native MIDI captures bind the source, physical endpoints, timing
authority and accepted revision before due-frame validation.

Save exports a verified current `accepted-constant-timing-qm-raw-v1` envelope
with original/mono provenance and exact binary64 values.
[`accepted_restore.py`](../../src/flitzis_looper/controller/accepted_restore.py)
gets a fresh runtime source, verifies complete source geometry and hashes,
and waits for fresh acknowledgement and derived refresh. Pending or rejected
restoration leaves Automatic unavailable. Ordinary analysis, legacy data and
saved overrides retain their respective authorities.

## Loop domain and BPM Lock

[`SourcePlayback`](../../rust/crates/looper/src/audio_engine/source_playback.rs)
derives phase from active output-frame counts within a binary64 rate epoch. In
[`SourceReadPlan`](../../rust/crates/looper/src/audio_engine/source_reader.rs)
a compatible accepted loop can wrap at its fractional musical period while
preserving integer physical markers. The final PCM knot interpolates to loop
zero at that fractional period. The nearest positive 1/16-beat duration must
divide a bar or be a whole-bar multiple, with the musical length within one
loaded frame of the physical length. Without a current acknowledged compatible
period, the reader wraps at the physical length. Dry, pitch-history and stem
readers share the source trajectory and require the
complete admitted context including the relevant Intro/Tail range. View local
zero never replaces source frame zero. A rounded length must not be folded into
playback rate to compensate for a seam.

BPM Lock divides source period by master output period when both are available.
Otherwise it uses global speed. The result is bounded by the supported rate
range.
It has its own pending/effective preparation and is independent of MultiLoop.
Selecting another PAD does not choose a new master. The timeline can bootstrap
phase once from a valid active source reference, without resetting output time
or moving playheads. Quantization selects a launch frame. Playback does not
follow changes in recorded source tempo or align sources continuously.

See the [waveform](../../openspec/specs/waveform-editor/spec.md),
[transport](../../openspec/specs/transport-timeline/spec.md) and
[source/stem alignment](../../openspec/specs/loop-source-stem-alignment/spec.md)
contracts.
