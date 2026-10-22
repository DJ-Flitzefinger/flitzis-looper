## Existing authority

The same current immutable Native SourceBeatmap revision supplies viewport anchors,
span endpoints and edits. SourceBeatmapSelection and its existing expected-revision,
content/source and private-delivery fences continue to own admission and publication.
Python consumes bounded DTOs; it does not read full map JSON or interpolate gaps.

## Inspection is independent of supported interpolation

The existing editor_view signature and DTO remain unchanged. Valid viewports first
select physically visible existing accepted anchors. The combined anchors/lines
limit remains at most 2048, and overfull views explicitly refuse rather than silently
truncating. An unsupported loop reference returns its existing unsupported_reference
status with the available anchors but no lines or loop-relative beat numbering.
Other projection refusals may retain an already bounded anchor list. Invalid input
or an excessive anchor count returns no points. Physical-domain checks remain separate.

The waveform renders each returned physical anchor as a muted short tick. The marks
indicate accepted source positions, not supported musical beats, verified downbeats,
calibrated detector confidence or source playback readiness. They never enable snap.
The existing Realtime-BPM OFF scalar-grid choice remains unchanged; position-only
inspection marks may overlay it without enabling variable musical grid lines.

## Position correction before coverage review

The span inspector offers Use first anchor and Use last anchor. Each resolves the
current revision-fenced span and fills the existing beat/sample fields from the
Native endpoint. Virtual endpoints have no physical sample and refuse this action.
This selection alone neither submits a job nor moves the playhead or loop markers.

MoveBeat first checks whether the requested beat is exactly an existing anchor.
Only insertion of a new beat still requires supported interpolation. Existing-anchor
position changes retain strict finite/source-sample/rate/ordering validation and the
same captured inverse. Existing sample-edit evidence records explicit positioning
independently of interval coverage and meter; the original anchor review classification
remains unchanged because a position edit does not certify musical accuracy. Raw
observations remain immutable and exact Undo restores the previous complete anchor.

## Limits and acceptance

This closes a concrete review/correction gap for existing maps. It does not produce
the missing automatically qualified full-track map. BeatThis pulse and downbeat
estimates alone do not prove quarter unit, count continuity or meter; those gates
and the renderer/onset/continuous/Human gates remain in the complete Goal.
