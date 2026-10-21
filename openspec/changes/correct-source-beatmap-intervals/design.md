## Span and inspector

An explicit selection uses existing accepted anchor indices `first < last < anchor_count`; it affects intervals `[first,last)`. Indices are meaningful only in the captured map revision. `editor_span(first_anchor_index,last_anchor_index)` returns revision/status, exact endpoint indices/source seconds/beat coordinates/sample indices, quarter-note difference, interval count and coverage/meter/downbeat summaries. It must work inside gaps without a supported loop reference. Derived immutable prefix metadata supplies bounded constant-time summaries; no full record parse or interval scan belongs in each UI frame. Invalid selections return an explicit unavailable DTO. Virtual anchors remain virtual and have no physical sample index.

## Count

`set_count` carries schema_version1, first_anchor_index, last_anchor_index and finite positive quarter_notes. Let L/R be the old endpoint beat coordinates, C the requested count and delta=C-(R-L). Preserve the first beat exactly, affine-map interior beat coordinates by `L+(B-L)*C/(R-L)`, set the final endpoint to `L+C`, and translate later anchors by delta. Source seconds, sample-edit provenance, raw detections, coverage and meter certainty stay unchanged. Validate all resulting finite/spacing constraints; never clamp. The UI states that this explicitly accepts proportional allocation inside the selected span.

When the count changes, selected interval downbeat labels become unknown because the old four-quarter lattice is generally incompatible with the relabeling. For suffix intervals, integral delta transports known phase modulo four without overflowing an integer label; fractional delta clears phase. Null never becomes zero. Preserve unaffected metadata exactly, and record this evidence invalidation in count lineage. An unchanged count preserves all phase fields.

## Independent interpretation

`set_meter` carries the same indices and meter_status unknown or quarter_note_4_4_verified. It does not change B/S or coverage. A changed meter clears its old downbeat label; unknown always clears it. Setting already verified 4/4 preserves that interval's known phase.

`set_downbeat` carries the indices and downbeat_label (exact signed i64 or null). Setting a label requires verified 4/4 across the selected intervals and does not enable meter. Clearing a label is allowed without inventing meter. With verified 4/4, d denotes the lattice d+4Z in absolute source quarter-note coordinates; d and d+4 are equivalent. Verified 4/4 plus null means known meter and unknown downbeat. Historical unknown-meter labels remain readable but cannot produce bar claims. Reduce i64 modulo four before floating-point conversion.

`set_coverage` carries indices, coverage trusted/uncertain/gap_unknown_count and reviewed_quarter_notes. Trusted or uncertain requires an explicit finite positive reviewed count exactly matching the current selected endpoint difference; the action accepts the displayed current allocation for every selected interval. Gap coverage needs no asserted count. Count and meter actions never promote coverage. Coverage edits use a distinct `coverage` lineage kind; old Native versions refuse this newer supported correction capability rather than interpreting it as another kind.

## Display, snapping and compatibility

Fine subdivision and quarter geometry remains phase-independent. A separate downbeat flag marks only verified known four-quarter phase. Coarse bar grids project each supported segment's actual phase; sixteen-quarter thinning does not assert phrase origins. Combined Native lines and anchors remain bounded by2048 with explicit refusal. Bar-sized grid snapping requires a known compatible phase and refuses unsupported crossings; fine snapping retains its existing later-tie geometry. Apply bars remains duration S(B(start)+4*bars), independent of known downbeat phase.

SourceBeatmap.EDITOR_CORRECTION_API_VERSION=2 covers the inspector and these operations. Portable and launcher checks require that exact integer capability and the inspector signature before App startup, without creating an Engine or doing Native work.

## Authority, Undo and errors

All actions use the original source/content/revision-fenced selection.edit job. Each immutable revision retains raw data, parent and distinct operation intent with an exact bounded inverse. Closed Undo validation checks exact shapes, deterministic current-state correspondence and the shared geometry validator; imported generic provenance is never executable authority. Undo restores predecessor beat bits and labels exactly rather than subtracting rounded deltas. Keep existing record/array/8KiB intent bounds.

Transient UI inputs and press/release targets are fenced by content, actual source, revision and selected range. Normal unsupported/readiness state stays visible; detailed failures use the original redacting console sink, including asynchronous map errors. No revision alone moves physical loop markers or changes audible scalar state. Same Copy independence, durable publication, cancellation/stale failure and model-free restore contracts remain mandatory.
