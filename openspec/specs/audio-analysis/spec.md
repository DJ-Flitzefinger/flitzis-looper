# audio-analysis Specification

## Purpose
To define background audio analysis for BPM, key, beat grid, and downbeat metadata used by UI display, snapping, and Rust timing metadata publication.
## Requirements

### Requirement: Primary BPM and optional key have independent admission
Configured primary-neural analysis SHALL admit full-source BPM independently of
optional key work at every pre-read and execution gate, retaining512MiB and all
source/request/cancellation/publication guards. Optional concurrent key admission
SHALL include the caller's retained interleaved PCM, additional held storage,
concurrent wire storage, bounded conversion/I/O buffers, FFT/Sinc workspace and
retained CQT responses/frontend. Complete Key PCM and octave signals SHALL use
job-owned file storage with bounded reads rather than whole-track widening or
octave heaps. The512MiB preparation allocation cap is not a process-tree RSS or
OS file-cache guarantee; cached model/ORT arenas remain separately owned.
No source cropping or limit increase SHALL substitute for independent admission.
Both current loader DTO routes SHALL keep the required key string and explicit
key_status ready/skipped_budget/not_requested/failed plus available key_error.
An actual completed unknown classification SHALL remain distinct from skipped work.

#### Scenario: Optional key exceeds preparation capacity
- **WHEN** mandatory full-source BPM fits but optional concurrent key work exceeds512MiB
- **THEN** BPM SHALL proceed while the key outcome explicitly reports skipped_budget
- **AND** a fresh source SHALL display unknown without publishing a detected key version
- **AND** same-source reanalysis SHALL preserve prior detected key, correction and numeric shifts

#### Scenario: Key fails, was not requested or lacks a metadata epoch
- **WHEN** optional key work fails, is unrequested or cannot reserve its metadata epoch
- **THEN** independently valid BPM SHALL remain publishable with an understandable component outcome
- **AND** prior key intent SHALL remain unchanged
- **AND** whole-request source replacement, cancellation and stale feedback SHALL retain their existing fences

#### Scenario: Matching successful key completion
- **WHEN** genuinely admitted key work successfully completes for its captured current source and request
- **THEN** completion MAY publish a detected source version and clear only the captured old correction
- **AND** admission alone SHALL NOT clear correction and a newer correction SHALL remain authoritative

### Requirement: Analyze Audio For BPM, Key, And Beat Grid
The system SHALL analyze a loaded audio sample to determine its BPM, musical key, and beat grid.

Configured primary-neural analysis SHALL export the complete immutable loaded PCM
once as arithmetic-channel-mean f32 LE mono at its actual loaded rate, preserving
frame zero, source identity and the full tail. Beat This SHALL consume that sealed
mono through its pinned worker frontend. KeyNet SHALL independently derive complete
44100-Hz mono with the existing delay-trimmed Rubato FFT conversion, then use the
existing pinned Sinc octave conversion and shared CQT/classifier math.
Neither branch SHALL crop or decode another source. Original interleaved PCM
SHALL remain available for later QM corroboration/fallback and range selection.

Admitted primary BPM and optional Key work SHALL execute concurrently off realtime;
the result SHALL be assembled after both finish and staged-input integrity and
cancellation are checked. Unconfigured scientific callers SHALL retain their shared
44100-Hz mono and QM-dsp/KeyNet path. File read/write/extent/nonfinite and scratch
failures SHALL report a failed Key component, never a fabricated classification.
All job-owned scratch handles SHALL close before cleanup on success, failure,
cancellation or stale completion.

The system SHALL represent the detected key as a musical-notation string (e.g., `"C#m"`) suitable for display in a professional audio application. When key detection fails or the audio is insufficient, the system SHALL use `"unknown"` as the key value.

#### Scenario: Analysis produces BPM, key, and beat grid
- **GIVEN** a pad has a loaded audio file
- **WHEN** the analysis workflow is executed for that pad
- **THEN** the system decodes the audio once and converts to mono
- **AND** the system runs BPM detection and key detection concurrently on separate threads
- **AND** the system produces a BPM value (float) from the BPM pipeline
- **AND** the system produces a musical key value in musical notation (e.g., `"C#m"`) from the key detection pipeline
- **AND** the system produces a beat grid containing beat times from the BPM pipeline
- **AND** the system produces downbeat times when they can be determined
- **AND** concurrent component execution is followed by integrity checks and any required BPM corroboration/fallback

#### Scenario: Analysis failure is reported
- **GIVEN** a pad has a loaded audio file
- **WHEN** analysis fails due to an unsupported or invalid audio signal
- **THEN** the system reports an error for that pad
- **AND** the previously stored analysis result (if any) remains unchanged unless explicitly cleared

#### Scenario: Key detection failure does not block BPM results
- **GIVEN** a pad has a loaded audio file
- **WHEN** the BPM pipeline succeeds but the key detection pipeline fails
- **THEN** the system returns the BPM and beat grid from the BPM pipeline
- **AND** the system uses `"unknown"` as the key value
- **AND** the error is logged for diagnostics

### Requirement: Analysis Can Be Triggered Automatically And Manually
The system SHALL run analysis automatically as part of the sample loading workflow, and SHALL allow the user to trigger analysis manually for an already-loaded pad.

Manual analysis SHALL enqueue an analysis-only background task without replacing or republishing the playback source. It MAY prepare analysis-specific mono/resampling from the captured immutable loaded PCM, without re-decoding the original file.

#### Scenario: Automatic analysis runs on load
- **WHEN** a sample load completes successfully for a pad
- **THEN** the system runs analysis for that pad before considering the load operation complete

#### Scenario: Analysis results are restored from project state
- **GIVEN** a project is loaded that contains persisted analysis results for a pad
- **AND** the corresponding sample file exists in the project's `./samples/` directory
- **WHEN** the sample is restored during project loading
- **THEN** the system restores the analysis results from project state without re-running analysis
- **AND** the restored results are available immediately for UI display and other features

#### Scenario: Manual analysis re-runs detection
- **GIVEN** a pad has a loaded audio file
- **AND** analysis results already exist for that pad
- **AND** the pad is not currently loading
- **WHEN** the user triggers "Analyze audio"
- **THEN** the system re-runs analysis and updates stored results for that pad

#### Scenario: Manual analysis is blocked while loading
- **GIVEN** a pad is currently loading
- **WHEN** the user attempts to trigger "Analyze audio" for that pad
- **THEN** the system blocks the request
- **AND** no analysis-only job is started for that pad


<!-- Added from add-rust-transport-timeline -->
### Requirement: Beatgrid Metadata Can Be Published To Rust Timing
The system SHALL make audio-analysis beatgrid and downbeat metadata available to Rust
playback timing as bounded, precomputed timing metadata.

Analysis, validation, and conversion of beatgrid vectors SHALL happen outside the audio
callback. The audio callback SHALL receive only fixed-size messages or preallocated state
updates that are safe to process in real time. The bounded metadata SHALL include a finite
non-negative per-pad phase anchor in seconds and SHALL NOT publish full beat-grid vectors into the
audio callback.

#### Scenario: Analysis results produce bounded timing metadata
- **GIVEN** a pad has analysis results with BPM, beats, downbeats, and bars
- **WHEN** the controller or background Rust code prepares playback timing metadata
- **THEN** it computes finite bounded values suitable for Rust transport use
- **AND** no beat detection or vector allocation is required in the audio callback

#### Scenario: Invalid timing metadata falls back safely
- **GIVEN** a pad has analysis metadata with no finite non-negative downbeat or beat anchor
- **WHEN** bounded playback timing metadata is prepared
- **THEN** the published phase anchor is `0.0` seconds
- **AND** the audio callback updates fixed-size state without panic, allocation, blocking, disk I/O, or GIL access

#### Scenario: Unloaded pad does not publish stale timing metadata
- **GIVEN** a pad has been unloaded
- **AND** the pad has a persisted negative grid offset from earlier editing
- **WHEN** controller code clears the pad BPM and analysis state
- **THEN** the control layer does not publish a stale negative pad timing anchor to Rust
- **AND** unload completes without raising an audio-engine timing metadata validation error

#### Scenario: Persisted analysis can restore timing metadata
- **GIVEN** a project restores persisted analysis results for a pad
- **WHEN** the pad is restored to the audio engine
- **THEN** the system can republish the pad's bounded beatgrid/downbeat timing metadata to Rust
- **AND** quantized playback can use the metadata without re-running analysis in the callback

