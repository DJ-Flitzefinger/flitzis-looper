## MODIFIED Requirements

### Requirement: Analyze Audio For BPM, Key, And Beat Grid
The system SHALL analyze loaded audio for BPM, beat/downbeat positions and musical key using
Beat This! 1.1.0 checkpoint `final0` with minimal postprocessing as the selected default for new
beat analysis after the explicit acceptance/cutover gate.

The system SHALL reuse the loader's immutable decoded PCM and derive one shared mono input
with the same source time-zero. The beat branch SHALL derive 22050-Hz input and the existing
Rust KeyNet branch SHALL derive 44100-Hz input directly from shared mono. Neither branch SHALL
re-decode the file or cascade its required-rate conversion through the other branch's input.
All preprocessing, inference and result validation SHALL execute outside the audio callback.

The system SHALL run available beat and key branches concurrently under the existing background
job lifecycle and assemble a typed result after each branch reaches a terminal outcome. The
branches SHALL have independent status/provenance. Key results SHALL use musical notation such
as `C#m`; a failed new key result SHALL use `unknown` without discarding successful beat output.

The system SHALL retain raw f64 detected times relative to the decoded source origin, record
model/preprocessing identity and distinguish raw predictions from accepted/manual beatmaps.
It SHALL NOT claim sample-accurate musical timing from the model's 20-ms prediction grid.
An unavailable selected backend SHALL NOT silently invoke qm-dsp or another checkpoint.

#### Scenario: Analysis produces BPM, key, and beat grid
- **GIVEN** valid loaded PCM and the verified selected worker/model are available
- **WHEN** new analysis runs after the default cutover
- **THEN** shared mono feeds the 22050-Hz Beat This and 44100-Hz Rust KeyNet branches
- **AND** the result includes beat/downbeat times, a versioned BPM summary and musical key
- **AND** each component records its own status and provenance
- **AND** neither pipeline re-decodes the file

#### Scenario: Analysis failure is reported
- **GIVEN** a pad has previously stored analysis
- **WHEN** empty or invalid shared audio prevents analysis
- **THEN** the request reports an error
- **AND** the previously stored result remains unchanged unless explicitly cleared

#### Scenario: Key detection failure does not block BPM results
- **GIVEN** the beat branch succeeds
- **WHEN** key detection fails or has insufficient audio
- **THEN** the beat result remains available
- **AND** the new key result is `unknown` with a separate failure/insufficient-data status
- **AND** diagnostics are emitted outside the callback

#### Scenario: Missing selected model preserves valid key output
- **GIVEN** the selected Beat This worker or checkpoint is unavailable
- **WHEN** analysis is requested on valid loaded audio
- **THEN** beat analysis reports unavailable without downloading or invoking another detector
- **AND** KeyNet may complete and publish its valid result
- **AND** existing accepted beat data remains explicitly retained, not relabeled as new success

### Requirement: Analysis Can Be Triggered Automatically And Manually
The system SHALL request analysis during normal sample loading and SHALL support manual
analysis of an already-loaded pad through the same selected-backend job path.

Manual analysis SHALL NOT re-run file decoding, playback resampling/channel mapping or sample
publication. It MAY prepare or reuse analysis-only mono/required-rate inputs from immutable
loaded PCM. Optional beat-worker/model availability SHALL NOT prevent a successfully decoded
sample from loading or playing. Setup/acquisition SHALL NOT run as part of an analysis request.

Project restore SHALL preserve stored analysis and manual BPM/grid/loop intent without
automatic reanalysis. Valid saved results SHALL remain usable without optional inference
runtime/model files; legacy results SHALL retain their legacy identity and precision. Manual
reanalysis SHALL create a new raw result without silently overwriting an accepted manual map.

#### Scenario: Automatic analysis runs on load
- **GIVEN** a valid sample has decoded successfully after the default cutover
- **WHEN** normal loading requests analysis
- **THEN** the same selected-backend job path analyzes its immutable loaded PCM
- **AND** beat and key components settle independently before analysis is complete
- **AND** loading or playing valid audio does not require installing optional beat support

#### Scenario: Automatic request settles without optional model
- **GIVEN** a valid sample decodes but the selected beat model is absent
- **WHEN** normal loading requests analysis
- **THEN** the sample can be published and played
- **AND** the analysis request settles with explicit beat-unavailable status
- **AND** no setup/download or silent legacy-detector fallback occurs

#### Scenario: Analysis results are restored from project state
- **GIVEN** a project contains analysis for its matching sample source
- **AND** optional Beat This runtime/model files are absent
- **WHEN** the project is restored
- **THEN** stored beat/key results and manual intent are restored without inference
- **AND** new-model provenance is not invented for legacy analysis

#### Scenario: Manual analysis re-runs detection
- **GIVEN** a pad is loaded and has an accepted manually corrected map
- **WHEN** the user triggers analysis
- **THEN** an analysis-only job reuses immutable loaded PCM
- **AND** successful raw results update with their own revision and provenance
- **AND** playback buffers, loop markers and the accepted manual map remain unchanged

#### Scenario: Manual analysis is blocked while loading
- **GIVEN** a pad is currently loading
- **WHEN** manual analysis is requested
- **THEN** the request is blocked
- **AND** no analysis-only job starts

## ADDED Requirements

### Requirement: Frozen Temporal Scoring Remains Separate From Musical Acceptance
The system SHALL provide an explicitly invoked offline temporal metric core for the frozen B2
protocol, using monotone one-to-one maximum-cardinality matching, then minimum summed absolute
timing error, then the lexicographically smallest reference/prediction index-pair sequence.
It SHALL evaluate beats and downbeats separately at 10, 20, 40 and 70 ms, with reference timing
uncertainty half-width limits of 2.5, 5, 10 and 10 ms respectively. It SHALL retain original
indices, all eligibility denominators, signed errors, absolute error distributions, missing/extra
counts and predeclared regional results. Predictions SHALL be excluded only by the supplied
predeclared nonmetrical regions, never by proximity to ineligible reference labels.

The core SHALL bound each complete event sequence to 250000 events and each matching to
1000000 eligible pairs, rejecting an exceeded bound without truncation or approximation.
Its longest matched run SHALL stop at an unmatched/ineligible reference, an intervening extra
prediction or a predeclared region boundary. Interval-distance diagnostics SHALL describe the
same point matches and SHALL NOT replace point-tolerance evidence. The core SHALL perform no
file access, inference, audio-device operation, playback adoption or project persistence.

The core SHALL identify its reference/candidate certification as unchecked and retain pending
musical acceptance and blocked default adoption. Input structure, synthetic tests and temporal
matches SHALL NOT certify independent labels, raw-artifact lineage, quarter counts, bar identity,
paired human correction burden, listening acceptance or default replacement. The separate
sealed-reference/artifact orchestration and all original B2 acceptance gates SHALL remain required.

#### Scenario: Broad uncertainty cannot erase a candidate extra
- **GIVEN** a metrical reference contains a label ineligible at the chosen tolerance
- **WHEN** the temporal metric core evaluates the complete candidate sequence
- **THEN** the ineligible reference is reported separately
- **AND** a prediction near it remains in the prediction denominator unless it belongs to a
  predeclared nonmetrical region

#### Scenario: Matching selects the frozen global optimum
- **GIVEN** several monotone one-to-one matchings meet the inclusive tolerance
- **WHEN** the offline core evaluates them
- **THEN** maximum count takes precedence over timing cost and exact timing cost over index order
- **AND** a tie retains the lexicographically smallest original index-pair sequence

#### Scenario: A metric result cannot release the new default
- **GIVEN** synthetic or structurally valid events produce perfect temporal scores
- **WHEN** the core returns its report
- **THEN** reference/candidate certification remains unchecked, musical acceptance pending and
  default adoption blocked
- **AND** no file, model, audio device, saved result or live timing is accessed or adopted

#### Scenario: Dense input exceeds the bounded scorer
- **GIVEN** the complete input would require more than 1000000 eligible pairs
- **WHEN** matching starts
- **THEN** it fails explicitly without returning a truncated or approximate score

### Requirement: Automatic BPM Summary Preserves Full Beat Evidence And Manual Intent
The system SHALL derive automatic BPM metadata from the selected backend's complete valid
beat results using a documented versioned estimation method and explicit beat-unit policy.
It SHALL preserve raw beat coordinates and distinguish aggregate BPM metadata from local
beat-interval timing. It SHALL NOT establish correctness by rounding toward an integer BPM
or replace variable source timing with the aggregate value for future mapped playback.

The system SHALL preserve existing manually entered or TAP-derived BPM overrides through
new automatic analysis of the same unchanged source. Insufficient or ambiguous beat/count
evidence SHALL have an explicit non-certified status rather than a claim of exact tempo.
The estimation method SHALL be validated on distant source intervals before the separate
default-cutover gate passes.

The estimation method SHALL evaluate a longer rhythmically stable central source region
as a candidate for pad-load BPM metadata against complete-track and distant-region evidence.
Its selection/weighting policy SHALL be versioned and frozen before held-out evaluation.
A central region with insufficient or changing rhythm SHALL NOT establish constant tempo
by its position alone; another supported region or explicit uncertainty SHALL be retained.

Comparable transient spans used for refinement SHALL retain their original source-time
anchors and explicit quarter-note counts. Snare-to-snare distance SHALL NOT imply one beat
without count evidence. The system SHALL retain selected-region provenance and uncertainty,
preserve complete adopted model input/raw results and assess half/double tempo, missing
events, syncopation and genuine local variation. Region selection SHALL NOT relocate the
grid origin or loop start, or flatten variable timing into one playback tempo.

The bounded diagnostic implementation SHALL freeze `selected-backend-bpm-v1` and
`representative-middle-region-v1` before private evaluation. Its complete estimate SHALL
use centered binary64 least squares on all assigned complete-sequence observations with
equal observation weights, and SHALL retain complete residuals and separate unchanged
G2 `constant-period-candidate-v1` robust/distant-window diagnostics. Default ordinal quarter
counts, half/double alternatives and supplied rational count assertions SHALL remain
unverified; fit quality SHALL NOT certify their musical truth. Explicit missing-count jumps
and excluded extras SHALL retain their original raw index and source-time coordinates.

Regional candidates SHALL be the complete-source middle `[20%,80%)` and each temporal
third. Regional fits SHALL reuse G2 numerical fitting, require at least 24 assigned and
retained observations, 30 seconds retained span, 60% window coverage, support in its
first/last 20%, no more than 10% raw exclusions or two consecutive exclusions, and affine
feasibility under the conditional lattice bound. The threshold SHALL remain below one
quarter of the fitted quarter-note period. Eligible middle SHALL win; otherwise longest
retained-span third SHALL win, then most inliers, then earliest window. Unsupported
candidates SHALL remain explicit; no eligible window SHALL yield no representative BPM.
Regional metadata SHALL NOT overwrite the complete estimate or complete-source status.

The diagnostic report SHALL distinguish the conditional 0.01-second detector-lattice
halfwidth and slope sensitivity from unestablished acoustic/count uncertainty. It SHALL
retain every raw beat/downbeat/logit array, local intervals, request/model identity and
complete loaded extent. It SHALL NOT supply a G3 timing bound, accepted revision, map or
default adoption. The diagnostic supervisor SHALL expose a summary only from validated
bounded final bytes after retirement and matching native finish acceptance. Cancelled,
stale, failed or oversize beat publications SHALL expose no summary. Metadata failure
SHALL remain explicit without stranding admission or replacing complete beat/key outcomes.

#### Scenario: Constant source tempo is assessed across the complete beat sequence
- **GIVEN** complete valid beat evidence with a consistent pulse interpretation
- **WHEN** automatic BPM metadata is computed after the cutover gate
- **THEN** the versioned method uses the complete evidence and is validated across distant intervals
- **AND** its method, beat-unit interpretation and uncertainty status are identifiable
- **AND** raw beat coordinates remain unchanged

#### Scenario: A stable fractional tempo remains valid
- **GIVEN** complete beat evidence supports a stable noninteger tempo
- **WHEN** the automatic summary is computed
- **THEN** the fractional value is retained under the versioned method
- **AND** proximity to an integer does not cause automatic rounding or declare correctness

#### Scenario: Local source variation is not flattened into timing truth
- **GIVEN** beat intervals vary across a source
- **WHEN** aggregate BPM metadata is produced
- **THEN** the original local beat timings remain available
- **AND** the summary does not claim those intervals are constant or activate mapped playback

#### Scenario: Automatic analysis preserves a tapped override
- **GIVEN** a pad has an existing manually entered or TAP-derived BPM override for its source
- **WHEN** new automatic analysis completes for that same unchanged source
- **THEN** the new automatic metadata does not overwrite that override
- **AND** manually clearing the override remains the explicit way to select automatic BPM

#### Scenario: A stable middle region is tested against an ambiguous intro and outro
- **GIVEN** a source has sparse or ambiguous intro/outro rhythm and a longer stable middle
- **WHEN** a representative region is evaluated for pad-load BPM metadata
- **THEN** its estimate is compared with complete-track and distant-region evidence
- **AND** its versioned selection and uncertainty remain identifiable
- **AND** full raw predictions and the original grid and loop origins remain unchanged

#### Scenario: An ambiguous middle does not establish tempo by location
- **GIVEN** the source middle contains a breakdown or changing tempo
- **WHEN** representative-region selection runs
- **THEN** another supported region or explicit uncertainty is retained
- **AND** central position does not certify a constant BPM for the source

#### Scenario: Comparable snares retain the musical interval count
- **GIVEN** two reliable snare anchors span a known number of quarter notes
- **WHEN** their elapsed source seconds support a tempo refinement
- **THEN** BPM uses that explicit count rather than assuming one beat between events
- **AND** missing or ambiguous event/count evidence cannot silently create half/double tempo
- **AND** original source anchors and raw beat results remain available

#### Scenario: A sparse-edge region remains scoped unverified metadata
- **GIVEN** only a longer middle has viable constant numerical evidence
- **WHEN** the diagnostic report selects that region
- **THEN** its estimate retains original raw indices and an unverified status
- **AND** complete-source coverage failure and the full estimate remain visible
- **AND** no accepted timing or live map is created

#### Scenario: Short sources have no silently shortened representative policy
- **GIVEN** no predeclared region has 30 seconds of retained evidence
- **WHEN** the complete sequence is summarized
- **THEN** the complete estimate remains available when numerically defined
- **AND** no representative region is selected

#### Scenario: A stale or oversize publication cannot retain a metadata summary
- **GIVEN** the background worker has complete raw beat results
- **WHEN** native finish rejects freshness or bounded final publication fails
- **THEN** the finished diagnostic snapshot exposes no BPM summary
- **AND** prior saved/manual/TAP/accepted timing remains unchanged

#### Scenario: Metadata failure retires the original job normally
- **GIVEN** complete validated beat/key publication succeeds but numerical metadata fails
- **WHEN** native finish accepts the matching request
- **THEN** full publication remains available with an explicit metadata error
- **AND** resources retire and later admission remains possible

### Requirement: Independent Acceptance Inputs Preserve Evidence Boundaries
The system SHALL provide an explicitly invoked offline preparation and validation workflow
for independent B2 references and paired human correction inputs without invoking inference,
reading candidate predictions during reference sealing or adopting results into project state.

Reference input SHALL bind the unchanged frozen corpus and scoring protocol to original source
hashes, complete loaded-rate mono PCM hashes, actual frame counts/rates and frame-zero origin.
The workflow SHALL require actual annotation audio and an independent human declaration,
full-span beat/count/bar/meter labels with bounded uncertainty, predeclared temporal regions,
recording-group/class certification and critical-downbeat identities or explicit absent features.
It SHALL check temporal coverage and eligible event denominators separately under the frozen
limits and reject cross-split recording-group contamination. Empty drafts or unavailable
annotation audio SHALL NOT become sealed references.

Reference and correction-order receipts SHALL bind exact input bytes and revisions. Paired
correction input SHALL retain the sealed reference, a predeclared balanced backend order,
matching human/tool/workflow identity, actual active human intervals and operation categories,
and corrected-legacy implementation provenance. Validation SHALL NOT manufacture labels,
human correction time, musical scores or acceptance. All files SHALL remain caller-selected
private workspace artifacts; preparation SHALL NOT overwrite frozen evidence. These operations
SHALL run outside the realtime audio callback and leave default routing unchanged.

Each correction phase containing operations SHALL retain its own positive recorded active
human time; another phase's measured time SHALL NOT satisfy that phase's evidence requirement.

#### Scenario: Empty independent annotation cannot establish acceptance
- **GIVEN** frozen corpus metadata and no complete independent human labels or annotation audio
- **WHEN** the offline workflow prepares and validates a reference draft
- **THEN** the draft lists required private inputs and remains incomplete
- **AND** no reference seal, musical pass or default cutover is produced

#### Scenario: Complete reference is sealed before candidate inspection
- **GIVEN** independently prepared complete labels, actual matching loaded PCM and split certification
- **WHEN** the offline workflow validates and seals the reference input
- **THEN** its receipt binds the exact source, PCM, protocol, annotation revision and input digest
- **AND** candidate predictions are not read and eligibility is distinct from musical acceptance

#### Scenario: Human correction comparison retains a predeclared order and endpoint
- **GIVEN** a sealed independent reference and a sealed order for both backends on held-out tracks
- **WHEN** actual human correction sessions are imported
- **THEN** the workflow rejects missing pairs, altered order or mismatched reference/tool/endpoint
- **AND** active intervals and edit categories remain observed human inputs rather than job durations
- **AND** valid session input alone does not certify comparative improvement

#### Scenario: Another phase cannot hide unmeasured correction operations
- **GIVEN** one correction phase has operations but no active time and another has measured time
- **WHEN** the paired correction input is validated
- **THEN** the unmeasured operation phase is rejected despite positive session-wide time
- **AND** genuine zero-operation zero-time baselines remain representable under the existing policy

### Requirement: Private Temporal Scoring Revalidates Complete Native Lineage
The system SHALL provide an explicitly invoked private offline scoring workflow that
revalidates full ReferenceSeal, bundle, protocol and inventory bytes, actual original
source and complete loaded PCM identities, independent declarations and recomputed
coverage before opening candidate plans or predictions.

The workflow SHALL bind approved native source/export/request identities or explicitly
approved byte-verified historical metadata to complete raw worker, component and final
envelope lineage through the existing strict readers. It SHALL reject duplicate fields,
duplicate candidates, mismatched source/model/request/PCM, incomplete arrays and unsupported
self-hashed lineage. Reports SHALL retain every original raw prediction and distinguish
missing inputs and original failed attempts. A missing or incomplete input SHALL NOT be
reported as complete diagnostics or acceptance.

Fresh native imports SHALL use separately approved fixed producer profiles binding actual
copy-first original bytes, complete finite loaded PCM, cold-source transform and generation,
finite resident window, complete mono export, actual worker request and frozen CPU model,
configuration, lock, producer implementation and executing native build identities.
They SHALL retain the retired export path separately from its retained complete copy,
require successful native finish and one identity-matched lossless ready-v2 completion,
and verify bit-exact full raw/component/finish/snapshot/completion arrays and actual retirement.
An installed extension unused by an embedded native probe SHALL remain a separate identity.
Historical rejected v1 evidence SHALL remain unchanged and rejected.

Explicit T01/T02 source-path aliases SHALL retain the unchanged frozen manifest and original
paths, bind actual replacement content by frozen size/SHA-256 and remain private workspace
artifacts. The workflow SHALL NOT derive labels from candidates, fit offsets, trim extents,
invent human observations or promote temporal metrics to musical/default acceptance.
It SHALL retain unchecked metric-core certification, pending musical/correction/count-bar
decisions and blocked adoption, operate outside realtime and leave live state unchanged.

#### Scenario: A changed reference blocks candidate inspection
- **GIVEN** a sealed bundle, source, PCM or recomputed coverage no longer matches its receipt
- **WHEN** private scoring is invoked
- **THEN** validation rejects before reading candidate plans or predictions
- **AND** no temporal score or adoption is created

#### Scenario: Complete historical native lineage remains explicit
- **GIVEN** approved byte-verified historical native metadata and matching full listening PCM
- **WHEN** worker/component/final artifacts retain identical complete arrays and identities
- **THEN** the workflow reports complete raw predictions and temporal diagnostics
- **AND** the retired export path and verified historical request reconstruction remain explicit
- **AND** musical truth and analyzer default acceptance remain pending and blocked

#### Scenario: A renamed frozen source preserves its original identity
- **GIVEN** T01 or T02 has an explicit private alias with its exact frozen historical path
- **WHEN** the replacement file matches the original byte size and SHA-256
- **THEN** the workflow verifies that content and retains both paths and alias bytes
- **AND** the frozen manifest and original reference provenance remain unchanged

#### Scenario: Missing inputs cannot produce full acceptance
- **GIVEN** a required reference/material/plan or selected candidate artifact is missing
- **WHEN** the workflow produces a private missing-input report
- **THEN** blocked or incomplete status and missing inputs remain explicit
- **AND** absent predictions are never fabricated and no musical/default pass is produced

#### Scenario: Fresh hardware-free native lineage replaces a rejected import selection
- **GIVEN** original complete v1 completion arrays lost bits and remain preserved
- **WHEN** a separately approved hardware-free cold-source/native-worker/v2 probe retains
  complete finite PCM and the exact producer, model, request and publication lineage
- **THEN** only the fresh profile is eligible for reference-first import after all checks pass
- **AND** neither engineering provenance nor successful inference supplies a reference seal,
  musical score, human acceptance or analyzer default promotion

### Requirement: Diagnostic Boundary Precedes Model And Default Activation
The system SHALL expose the loaded-PCM and real local worker boundary only through explicitly
invoked diagnostic analysis until the separate default-cutover acceptance gate is satisfied.

The diagnostic boundary SHALL preserve normal automatic/manual analysis routing, project
persistence, manual grids and playback buffers. It SHALL export complete mono float32-LE PCM
at the loaded sample rate with origin zero, derive key input directly at 44100 Hz and derive
Beat This input independently at 22050 Hz using its pinned reference frontend. An unconfigured worker
SHALL report unavailable without acquiring dependencies or weights. A diagnostic completion
envelope SHALL NOT be adopted as accepted project analysis or relabel legacy saved results.

#### Scenario: Diagnostic request has no installed beat worker
- **GIVEN** an already-loaded pad and no configured optional beat runtime
- **WHEN** an explicit diagnostic request starts
- **THEN** native shared-mono preparation reuses the immutable loaded source
- **AND** beat status is unavailable while key analysis may finish independently
- **AND** normal analysis routing, playback buffers and saved grids remain unchanged

#### Scenario: Diagnostic output does not activate the selected backend
- **GIVEN** a validated diagnostic component-result envelope
- **WHEN** the existing background event path reports completion
- **THEN** the envelope remains diagnostic data
- **AND** no model acquisition, default routing switch or accepted-map adoption occurs

#### Scenario: Real worker preserves reference preprocessing and source extent
- **GIVEN** complete shared mono at its actual loaded rate and a verified local worker
- **WHEN** diagnostic beat inference runs
- **THEN** direct full-buffer soxr HQ conversion and the pinned centered log-mel frontend
  preserve the reference time-zero and rounded resampled tail without fitted offsets or trimming
- **AND** all model logits are retained within declared size limits
- **AND** detected positions at or beyond the exclusive original source end are omitted
- **AND** clips too short for reference reflect padding fail explicitly without alternate preprocessing

### Requirement: Beat Analysis Jobs Preserve Identity And Independent Outcomes
The system SHALL validate request, source, generation and model identities before atomically
publishing component outcomes through the existing background event path. It SHALL distinguish
ready, unavailable, failed and cancelled attempts from any previously retained accepted result.

The system SHALL bound pending jobs, PCM transfer/output sizes and worker resources; reject
oversize work explicitly; and support cancellation/timeout without blocking UI or callback.
Stale results SHALL NOT overwrite a replacement source or restored/manual state. Temporary PCM,
worker shutdown and final resource destruction SHALL be handled outside the callback.

Worker ownership SHALL include interpreter-launcher descendants. On Windows, the process tree
SHALL be contained before worker code starts. Cancellation, timeout, failure and normal launcher
exit SHALL retire every remaining owned descendant before the beat process slot or borrowed PCM
is released. Launcher exit or a closed stdout pipe alone SHALL NOT prove worker retirement.

#### Scenario: Unresponsive cancelled beat work is retired within a bounded lifecycle
- **GIVEN** a cancelled beat job does not acknowledge cooperative cancellation
- **WHEN** the configured cancellation deadline expires
- **THEN** process supervision terminates the unresponsive worker outside the callback
- **AND** beat-owned temporary PCM and job resources are retired after their readers stop
- **AND** beat cancellation is not reported as complete while those beat resources remain active
- **AND** valid playback and other accepted analysis remain available

#### Scenario: Beat termination does not falsely complete a running key job
- **GIVEN** the beat worker has stopped but the same request's native KeyNet call is still running
- **WHEN** cancellation status and resource ownership are updated
- **THEN** the key work remains explicitly retiring and its PCM remains reference-owned
- **AND** the whole request is not reported terminal until both branches actually settle
- **AND** bounded key slots and retained-byte limits apply backpressure to subsequent work
- **AND** stale key output cannot publish and the UI/audio callback does not wait for that call

#### Scenario: Windows interpreter launcher owns an inference child
- **GIVEN** the optional environment's launcher starts inference in a child process
- **WHEN** analysis is cancelled, times out or the launcher exits
- **THEN** the supervisor terminates any remaining worker-owned descendants
- **AND** process-tree exit is confirmed before releasing beat admission and borrowed PCM
- **AND** unrelated application processes remain outside that ownership

#### Scenario: Cancelled worker completion cannot overwrite a new source
- **GIVEN** a beat job is cancelled because its pad was unloaded or replaced
- **WHEN** the worker later returns a valid-looking result for the old request
- **THEN** publication rejects its stale identity
- **AND** the new source and its analysis remain unchanged

#### Scenario: Component results publish atomically
- **GIVEN** valid key output and an unavailable beat outcome belong to the current request
- **WHEN** the result envelope is validated
- **THEN** key output, beat status and explicitly retained prior beat data publish together
- **AND** observers never see a fabricated successful beat result

#### Scenario: Oversize audio is not silently truncated
- **GIVEN** a request exceeds configured worker PCM/resource limits
- **WHEN** preflight checks the request
- **THEN** analysis reports an explicit limit failure
- **AND** it does not analyze a shortened file as if it were the whole track
- **AND** successfully loaded audio remains playable

### Requirement: Diagnostic PCM Staging Bounds Each Actual Ownership Stage
The system SHALL stream the complete immutable loaded source to one shared loaded-rate mono
float32 little-endian file, using an arithmetic channel mean accumulated in f64 and rounded
to f32 once per frame, without allocating a complete loaded-rate mono or key-input copy.

The export SHALL preserve frame zero, leading silence, every source frame and request/source
identity. Nonfinite input, incomplete frames, invalid metadata, I/O failure or cancellation
SHALL prevent a prepared-success outcome. The native analysis owner SHALL retain the source
pin throughout export and release it outside the callback only after a complete flushed file
and a readable native key handle are available. Releasing this analysis pin SHALL NOT replace,
mutate or release independent playback ownership.

Native PCM admission SHALL enforce the unchanged 512-MiB cap against the maximum simultaneous
ownership at either stage: retained interleaved source plus bounded export buffers, or complete
44100-Hz key output plus bounded read/resampler PCM buffers including converter delay/tail
capacity. The final key vector SHALL contain exactly the ceiling-derived frame count without
a second full delayed-output allocation. Every still-owned PCM allocation or source pin SHALL count in its live stage; none
SHALL be excluded merely because the data is immutable or shared. The complete exported file
SHALL independently remain at most 512 MiB. FFT scratch, CQT/ORT workspace and worker model
memory remain outside this PCM-only cap; playback ownership remaining after pin release SHALL
remain part of actual application/combined RSS measurements.

The job SHALL retain one active-or-retiring admission and zero pending queue. Native file
handles SHALL close outside the callback after key readers settle; the export and containing
job directory SHALL remain owned until both native key and the owned beat process tree have
actually stopped reading. Cancellation or stale source identity SHALL immediately invalidate
publication while a non-preemptible KeyNet call, worker or failed cleanup remains retiring.
Preparation, resampling, file cleanup and final publication SHALL remain outside the callback.

#### Scenario: Long loaded track fits only with separate ownership stages
- **GIVEN** a complete 96000-Hz stereo source would exceed 512 MiB if retained with full mono
  and key-input copies
- **AND** its source-plus-export-buffer stage, key-output-plus-bounded-buffer stage and complete
  export each fit their unchanged limits
- **WHEN** diagnostic preparation and key analysis run
- **THEN** bounded export creates every mono frame with the existing channel-mean bits
- **AND** native source ownership ends after successful export before key output is allocated
- **AND** both branches consume that complete mono source without shortening the track
- **AND** independent playback remains valid and its memory stays visible in RSS evidence

#### Scenario: Live ownership cannot evade the PCM cap
- **GIVEN** retained source plus bounded export buffers or full key output plus bounded buffers
  exceeds 512 MiB, or the complete mono file exceeds its independent 512-MiB limit
- **WHEN** native admission checks the required stages
- **THEN** it rejects the complete request explicitly before unbounded preparation
- **AND** it does not remove live source ownership from accounting, raise the cap or trim audio

#### Scenario: Export failure never starts readers on a partial source
- **GIVEN** an export fails or is cancelled before the complete file has been flushed
- **WHEN** the request retires
- **THEN** neither branch receives a successful prepared input for the partial file
- **AND** partial files and analysis ownership retire outside the callback
- **AND** playback and saved analysis remain unchanged

#### Scenario: Cancelled key reader retains its file and admission
- **GIVEN** both branches started from the complete staged file and KeyNet is still running
- **WHEN** the source is replaced or the request is cancelled after the beat worker retires
- **THEN** late publication is rejected while the key call and its PCM remain owned
- **AND** the file and job directory remain until the key reader has also settled
- **AND** subsequent work remains blocked by the occupied slot until actual cleanup completes

### Requirement: Diagnostic Publication Preserves Complete Predictions Losslessly
The system SHALL publish every successful diagnostic beat result with all raw beat/downbeat
positions and logits, preserving each validated binary64 value and the component's identity,
model provenance and independent key outcome.

The final schema-version-2 envelope SHALL store the four prediction arrays inline as canonical
padded standard Base64 of uncompressed IEEE-754 little-endian binary64 bytes, identified by
`float64-le/base64`. The worker request/response SHALL remain schema version 1. Readers SHALL
retain support for schema-version-1 final envelopes containing numeric arrays. The complete
final envelope SHALL remain limited to 1 MiB, worker responses to 8 MiB and each prediction
array to 250000 values. No truncation, downcasting, quantization, compression or external
artifact reference SHALL be used to bypass these limits.

Validation SHALL bound encoded and decoded extents, reject malformed/noncanonical Base64,
partial binary64 values, unknown encodings, nonfinite values, mismatched logit lengths and
positions outside the source or not strictly increasing before atomic native publication.
Packing, decoding, validation and cleanup SHALL execute outside the audio callback. The
existing request identity lock, actual worker/key retirement and single-admission policy SHALL
remain authoritative. A final result that still exceeds 1 MiB SHALL report an explicit beat
failure while preserving the independent key result; successful publication SHALL never
mean that any arrays were discarded. Diagnostic snapshot/event decoding SHALL use the same
versioned reader without requiring the retired PCM file or optional model installation.

#### Scenario: Complete long-worker result fits the unchanged final limit
- **GIVEN** complete validated worker predictions exceed 1 MiB as JSON numeric text
- **AND** their complete binary64/Base64 final envelope fits within 1 MiB
- **WHEN** both analysis branches and their resources retire
- **THEN** one validated completion publishes all positions and logits losslessly
- **AND** the event and snapshot retain model/source/request identity and independent key status
- **AND** default analysis routing and saved/manual grids remain unchanged

#### Scenario: Packed output remains oversize
- **GIVEN** complete packed predictions still exceed the 1-MiB final envelope limit
- **WHEN** the supervisor assembles the final result
- **THEN** beat status reports an explicit publication-limit failure
- **AND** independent key output is retained without a truncated beat success

#### Scenario: Corrupt or stale packed results cannot publish
- **GIVEN** a final packed envelope has malformed bytes, invalid values or stale identity
- **WHEN** native publication validates it
- **THEN** no successful completion for that result is emitted
- **AND** cancellation/source replacement still suppresses otherwise valid late results
- **AND** admission remains occupied until actual resources retire

#### Scenario: Diagnostic result remains readable after scratch cleanup
- **GIVEN** a complete diagnostic result has published and temporary PCM has been removed
- **WHEN** the versioned reader reads the snapshot or exported envelope without model files
- **THEN** all prediction values and component provenance are recovered exactly
- **AND** no scratch artifact, inference or reanalysis is required
