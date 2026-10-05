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

#### Scenario: Selected analysis produces independent beat and key results
- **GIVEN** valid loaded PCM and the verified selected worker/model are available
- **WHEN** new analysis runs after the default cutover
- **THEN** shared mono feeds the 22050-Hz Beat This and 44100-Hz Rust KeyNet branches
- **AND** the result includes beat/downbeat times, a versioned BPM summary and musical key
- **AND** each component records its own status and provenance
- **AND** neither pipeline re-decodes the file

#### Scenario: Invalid shared audio retains previous analysis
- **GIVEN** a pad has previously stored analysis
- **WHEN** empty or invalid shared audio prevents analysis
- **THEN** the request reports an error
- **AND** the previously stored result remains unchanged unless explicitly cleared

#### Scenario: Key failure does not discard valid beats
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

#### Scenario: Automatic request settles without optional model
- **GIVEN** a valid sample decodes but the selected beat model is absent
- **WHEN** normal loading requests analysis
- **THEN** the sample can be published and played
- **AND** the analysis request settles with explicit beat-unavailable status
- **AND** no setup/download or silent legacy-detector fallback occurs

#### Scenario: Stored results restore without model installation
- **GIVEN** a project contains analysis for its matching sample source
- **AND** optional Beat This runtime/model files are absent
- **WHEN** the project is restored
- **THEN** stored beat/key results and manual intent are restored without inference
- **AND** new-model provenance is not invented for legacy analysis

#### Scenario: Manual analysis preserves playback and manual map
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
The system SHALL expose the B1a loaded-PCM and worker boundary only through explicitly invoked
diagnostic analysis until real-model and default-cutover gates are satisfied.

The diagnostic boundary SHALL preserve normal automatic/manual analysis routing, project
persistence, manual grids and playback buffers. It SHALL export complete mono float32-LE PCM
at the loaded sample rate with origin zero, derive key input directly at 44100 Hz and leave
the actual Beat This 22050-Hz frontend to the real-reference stage. An unconfigured worker
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

### Requirement: Beat Analysis Jobs Preserve Identity And Independent Outcomes
The system SHALL validate request, source, generation and model identities before atomically
publishing component outcomes through the existing background event path. It SHALL distinguish
ready, unavailable, failed and cancelled attempts from any previously retained accepted result.

The system SHALL bound pending jobs, PCM transfer/output sizes and worker resources; reject
oversize work explicitly; and support cancellation/timeout without blocking UI or callback.
Stale results SHALL NOT overwrite a replacement source or restored/manual state. Temporary PCM,
worker shutdown and final resource destruction SHALL be handled outside the callback.

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
