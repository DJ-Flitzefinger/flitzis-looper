## ADDED Requirements

### Requirement: Unsupported intervals remain explicitly editable
The editor SHALL offer revision-fenced existing-anchor span selection and bounded Native diagnostics independently of supported loop-reference geometry. Native summaries MUST expose exact endpoint/count/coverage/meter/downbeat state without full JSON parsing or per-frame full-map scans in Python. Invalid indices SHALL refuse without mutation; virtual endpoints MUST NOT become readable sample indices.

#### Scenario: A gap covers the loop reference
- **WHEN** the performer selects its existing anchor span even though the viewport reference is unsupported
- **THEN** its true interval metadata and accepted endpoint coordinates remain inspectable and explicitly correctable
- **AND** no scalar fallback or inferred gap interpolation is displayed as supported geometry

### Requirement: Count repair preserves physical source coordinates
Count repair SHALL explicitly accept a positive quarter-note count between two existing selected anchors, affine-relabel interior beats and translate only subsequent beat coordinates. It MUST preserve all source-position/sample/raw bits, coverage and meter certainty and validate finite strict monotonic geometry. Changed selected downbeat phases SHALL become unknown; downstream known phase SHALL be transported modulo four only for integral beat shifts and otherwise become unknown. Unchanged count and unaffected metadata MUST remain exact.

#### Scenario: Missing beats are corrected over a selected span
- **WHEN** its performer-reviewed quarter count changes
- **THEN** local grid and source BPM follow the corrected B/S geometry while all source samples remain fixed
- **AND** raw observations and unsupported coverage stay unchanged, with dependent phase invalidation recorded

### Requirement: Meter downbeat and coverage are independent corrections
The editor SHALL expose separate explicit meter, downbeat and coverage actions. Verified 4/4 and unknown downbeat MUST remain distinct. A known downbeat d SHALL denote d+4Z in absolute quarter notes, using exact i64 phase before floating-point conversion. Setting downbeat MUST NOT enable meter; changing meter invalidates its old phase. Coverage trusted or uncertain SHALL require an explicit exact assertion of the displayed current quarter-count allocation, including removal of unknown-count gaps. Count or meter alone MUST NOT grant supported coverage. Interpretation edits MUST NOT change B/S geometry.

#### Scenario: A gap receives a count then a trusted interpretation
- **WHEN** the performer corrects its count
- **THEN** the gap remains unavailable
- **AND** only a separate explicit coverage action accepting its current count can make it trusted

#### Scenario: Meter is known but downbeat is not
- **WHEN** a verified 4/4 interval has no downbeat label
- **THEN** four-quarter durations remain supported but bar-phase claims and bar-sized snapping remain unavailable
- **AND** an explicit downbeat correction changes neither source positions nor local BPM

### Requirement: Visible bar phase shares native geometry
Known downbeats SHALL be distinguished from fine quarter emphasis, and coarse bar projection SHALL use each interval's actual phase rather than zero-origin thinning. Fine grid geometry SHALL remain unchanged by integral phase selection. Native bar snapping MUST require compatible known phase and supported coverage. Combined viewport anchors and lines SHALL remain bounded by2048 with explicit refusal. Loop numbering remains1+B(source)-B(loop_start), and Apply bars remains a duration operation.

#### Scenario: A nonzero downbeat is viewed at coarse zoom
- **WHEN** its supported interval is projected with four- or sixteen-quarter spacing
- **THEN** the visible bar lines retain its true four-quarter phase
- **AND** unknown meter or phase cannot acquire fabricated bar emphasis

### Requirement: Interpretation edits retain closed undo and publication ownership
All new corrections SHALL reuse the existing source-qualified Native job, immutable artifact, Config-before-owner selection and retirement paths. Exact bounded inverses MUST restore beat bits and interpretation metadata through sequential closed Undo, preserving operation kinds and refusing malformed/imported generic inverse authority. Source/content/revision/range replacement between mouse press and release MUST NOT redirect an action. Copy isolation and fresh model-free restore SHALL retain the corrected geometry and metadata. Detailed failures SHALL use the existing console sink; no edit alone moves physical loop markers or changes scalar audio, master or other PADs.

#### Scenario: Mixed corrections on a copy are undone and reopened
- **WHEN** count, meter, downbeat and coverage revisions complete on the copy and are sequentially undone or restored in a fresh process
- **THEN** each exact predecessor geometry/interpretation is retained without modifying the original PAD or its markers
- **AND** stale work, save failure or cancellation preserves the selected predecessor through the original owners

### Requirement: Correction capability is checked before startup
Launcher and Portable checks MUST require the exact integer SourceBeatmap.EDITOR_CORRECTION_API_VERSION2 and the bounded inspector signature without constructing an Engine or invoking jobs.

#### Scenario: An older Native exports the previous method signatures
- **WHEN** it lacks the correction capability or exposes an incompatible version
- **THEN** startup refuses before opening the App instead of accepting an editor whose new actions cannot execute
