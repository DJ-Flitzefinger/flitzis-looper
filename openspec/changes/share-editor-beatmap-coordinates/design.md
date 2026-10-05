## Proposed scope

Draft only. Build on the versioned offline foundation. This slice integrates display/snapping
and explicit correction; it does not drive audio from the new map.

Render visible musical lines from bounded native queries keyed by map revision, loaded rate,
viewport and resolution. Snapping and beat labels use that same evaluator. Python chooses UI
intent and persists accepted records, not an independent interpolation implementation.

The user confirmed that a variable track needs a current local BPM display (2026-10-05).
Query local quarter-note slope at the current/retained source playhead from the same native
map evaluator: source BPM = 60 * dB/ds, with s in seconds. Segment selection at exact anchors
follows that evaluator's versioned boundary convention. Distinguish source-local BPM, master
target BPM and an optional track summary. Unsupported/uncertain coverage stays explicit.
Under future SYNC, source-local tempo can differ from rendered target tempo; label those
domains instead of implying two conflicting current output tempos. Display cadence/rounding
is presentation only and never drives the map-based source trajectory. Tests cover segment
crossings, seeks, wraps, paused playheads and pending versus live map revisions.

The common coordinate is musical beat position, not identical horizontal pixels. In a musical
view, source position is projected relative to fixed beat lines. Existing source-time waveform
views may show nonuniform line spacing. Both use the same transform and explicitly identified
view; a view change cannot change master time or source data.

Keep a signed source alignment translation distinct from local anchor edits and the future
creative phase offset in beats. Positive existing sample offset moves the grid origin later
in source time; preserve that convention in scalar migration and label the equivalent source
motion correctly. Whole-map translation preserves inter-beat intervals. Local edits must keep
strict monotonicity, confidence/coverage and meter labels coherent.

Manual BPM remains scalar-mode intent unless the user explicitly selects a defined map transform.
Do not flatten an accepted tempo curve as an incidental consequence of editing a BPM display.
Analysis refresh must not overwrite accepted manual corrections.
After Beat This migration, expose source-sample local anchor movement and explicit beat-count/
downbeat correction as distinct operations. Optional local onset refinement starts disabled in
reference comparisons, retains raw position/method/displacement, and cannot force a syncopated
or silent beat onto the nearest transient. Refinement/manual correction does not require changing
the global clock. At low zoom the drawing may aggregate lines; storage precision stays unchanged.
Neither the existing source-key metadata correction nor future semitone transposition moves grids.

Build new revisions on the control side and show which revision is displayed. In this slice
variable maps remain editor/diagnostic-only, even after stop/retrigger. Never claim editor
revision N is already driving scalar audio. Future live adoption may begin at a safe start
boundary and requires its own
identity, preparation and transition proof. Source-time loop markers persist exactly until a
deliberate resnap/edit; a map change alone must not move them.

This draft's MODIFIED waveform/loop requirements scope the older scalar rules by mode and
deliberately supersede the corresponding grid requirement from preserve-manual-grid-phase only
when this editor change is implemented. Preserve that change's signed-origin scalar behavior.
Map-mode auto-loop operations add 4*bars in beat space and invert the shared map; merely accepting
a new revision does not recalculate saved marker positions. Existing explicit loop edits still
publish physical regions immediately; this is distinct from adopting mapped playback.

Use controller/evaluator tests for line/snap agreement, signed offsets, missing coverage,
meter ambiguity, stale viewport data and preserved master/other-pad state. Keep visual QA focused
on labels and pending revision state. Rollback selects the preserved scalar view/intent.
