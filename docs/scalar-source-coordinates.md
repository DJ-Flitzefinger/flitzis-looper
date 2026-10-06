# Scalar source coordinates

G1 shares the scalar editor projection and preserves long-file loaded-frame
addresses. Automatic tempo estimation and variable-map playback/SYNC remain pending.

## Domains and authority

| Value | Domain | Owner/consumer |
| --- | --- | --- |
| Original decoded frames/rate | Original file | Decoder/resampler; not editor offset units. |
| Loaded frames/rate | Resampled source at engine output rate | Waveform, markers, source reader and same-source stems. |
| Source seconds | Binary64 from loaded source zero | Saved intent, waveform X/query, seek and playhead APIs. |
| Source beat | Continuous scalar quarter-note coordinate | Pure Rust SourceGrid exposed as control-only ScalarSourceGrid. |
| Grid origin | Signed source reference | Rounded activity/legacy base plus signed loaded-frame offset, independent of loop start. |
| Physical marker | Integer loaded frame | One absolute-boundary rounding, then existing source clamps. |
| Output frame | Permanent Rust output clock | Transport/scheduler, separate from source addresses. |

Python resolves manual/TAP BPM before detected BPM. The control evaluator accepts
the binary64 period `60/effective_bpm`; native live timing still uses its accepted
binary32 BPM/rate parameters promoted for arithmetic. This shares arithmetic;
whole-engine accepted-period/revision unification remains G3.

## Projection and markers

`ScalarSourceGrid(origin_s, seconds_per_beat)` exposes `beat_at_source`,
`source_at_beat` and `source_after_beats`, reusing pure Rust source-grid arithmetic
without stream state, scheduling or source reads. Invalid construction raises
`ValueError`; nonfinite/overflow projection returns unavailable. A focused Python
scalar wrapper supplies it to visible lines, snapping and automatic loop ends.

Visible lines retain fractional frames. The finest musical snap is 1/16 beat.
Grid midpoint selection and physical marker rounding retain Python nearest-even
ties. Each physical boundary is evaluated absolutely and rounded once. Auto-loop
endpoints advance the original selected start by `4*bars` beats before start/end
are independently rounded. Off-grid starts retain their phase; labels use
`1+B(s)-B(loop_start)` and do not define the origin. Source clipping still applies.

## Public address precision

Native waveform bounds and X are binary64; amplitudes remain binary32. Raw X is
absolute loaded frame divided by loaded rate. Envelope buckets use integer frame
boundaries. A tightly bounded floating-point tolerance recovers exact boundaries
supplied as `n/rate`, preventing division noise from expanding a one-frame query.
Other fractional bounds retain floor-start/ceil-end coverage and source clipping.
Virtual negative view space adds no PCM.

Seek commands and playhead telemetry retain binary64 seconds through Rust messages,
PyO3 and Python. Seek still rounds to a valid physical frame and uses the existing
intro/loop/tail policy. Playhead still reports the floored next-source cursor.
Loaded duration also retains binary64 seconds so Python clipping does not narrow
the endpoint. Command/telemetry fields remain fixed-size.

ImPlot requires matching X/Y array types. Plotting promotes the small visible
amplitude arrays to binary64 while retaining X. Decoding/querying stays outside
rendering and the callback.

## BPM presentation and publication

Compact summaries may round. Tooltips and edit initialization disclose full
effective precision. Untouched or reverted edits publish no override/speed update
on Enter/focus loss. Intentional fractional edits, TAP and clear retain their
authority. MIDI runtime metadata and its signature use the authoritative effective
BPM resolver. Its signature also retains source path, loaded duration/rate and
the exact effective endpoints used for publication; future accepted timing
revisions must extend this identity.

## Remaining timing work

The automatic metronome estimate remains `120.00128936767578`, so its scalar slope
persists. G2 provides offline supported count/region/period fitting and uncertainty.
G3a adds an [immutable accepted revision and control-only guard](accepted-constant-timing.md).
G3b must connect actual current-pad validity and all consumers; G3c must separately
verify musical versus physical loop periods. Numerical coordinates do
not prove audible DSP/device alignment or musical model acceptance. See
[the diagnosis](grid-timing-diagnosis.md) and [the design](beatmap-sync-design.md).
