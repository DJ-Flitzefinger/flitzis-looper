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
