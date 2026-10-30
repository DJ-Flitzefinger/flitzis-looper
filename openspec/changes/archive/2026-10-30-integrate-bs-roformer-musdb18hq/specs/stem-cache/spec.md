## ADDED Requirements

### Requirement: Separator Selection Is Bounded And Captured
The system SHALL persist a bounded choice between Demucs htdemucs and BS-RoFormer
MUSDB18HQ and capture that choice in each immutable admitted generation request.
Existing projects SHALL default to Demucs; selection SHALL affect future jobs only
and SHALL preserve existing prepared stems and model-free cache restoration.

#### Scenario: Selection changes while a job runs
- **GIVEN** a BS-RoFormer job has been admitted
- **WHEN** the performer selects Demucs in Settings
- **THEN** the admitted job still uses BS-RoFormer and the next job uses Demucs
- **AND** current playback and complete cached sets remain eligible

#### Scenario: Restore with unavailable selected model
- **GIVEN** a valid source-bound complete cached set and no installed selected model
- **WHEN** the project restores
- **THEN** restoration uses the common native preparation and acceptance path
- **AND** it neither initializes nor downloads any separator model

### Requirement: Exact MUSDB18HQ Model Runs Offline
The system SHALL run the pinned upstream four-source BS-RoFormer MUSDB18HQ network,
release asset configuration and checkpoint outside realtime processing, verify
installed asset identities before loading, and reject missing or corrupt models
without substituting another model or downloading from the generation path.
CUDA auto policy SHALL try CUDA when available and MAY retry the same model on CPU.

#### Scenario: Actual model separation
- **GIVEN** the verified MUSDB18HQ assets and compatible runtime are installed
- **WHEN** a stopped loaded pad requests this separator
- **THEN** actual network inference produces vocals, drums, bass and other
- **AND** other maps to melody and the aligned components produce instrumental
- **AND** all five artifacts match the loaded sample shape

#### Scenario: Missing or altered checkpoint
- **WHEN** the selected checkpoint is absent or fails its pinned digest
- **THEN** generation fails before deserialization outside the callback
- **AND** existing full-mix and accepted prepared playback remain available

### Requirement: Separators Share Bounded Publication
The system SHALL use the existing source-ticket, private-generation, lease,
complete-set integrity and native-ACK publication path for both separators, keep
offline generation independent of playback under E11-05 immutable source leases and bound admission to two workers and 32 queued jobs. Active different-set adoption SHALL require separately proved current voice/history/window permits and own native ACKs; current runtime guards SHALL stay closed until that proof.

#### Scenario: Late obsolete completion
- **GIVEN** a source or timing ticket is superseded during model inference
- **WHEN** either separator completes
- **THEN** the obsolete job cannot replace the current set or make controls available
- **AND** its private artifacts retire only after actual readers release their leases

#### Scenario: Complete set waits for native acceptance
- **WHEN** either separator finishes a current complete generation
- **THEN** shared promotion verifies the complete set
- **AND** controls remain unavailable until native acceptance

#### Scenario: Admission exceeds a bound
- **WHEN** the two-worker, 32-queued-job admission bound is exhausted
- **THEN** admission fails with a bounded generation error before processing
- **AND** current effective audio remains available

### Requirement: Explicit PCM Buffers Are Bounded
The system SHALL cap explicit live PCM buffers in the BS-RoFormer worker and shared
alignment at 1 GiB maximum simultaneous scratch per job before allocation, stream complete source/five-stem postprocessing and fresh warm integrity verification without charging total file extent to that scratch bound and preserve existing native
cold/preparation caps. This cap SHALL NOT be presented as total Demucs neural
inference memory or whole-process RSS.

#### Scenario: Allocation exceeds the PCM bound
- **WHEN** BS-RoFormer or shared alignment would exceed its 1 GiB simultaneous scratch PCM bound
- **THEN** it fails with a bounded generation error before that allocation
- **AND** current effective audio remains available

### Requirement: Separation Stays Outside Realtime Processing
The system SHALL keep model loading, inference, I/O, Python/GIL/UI calls, blocking
locks, unbounded loops and heavy allocation outside the realtime callback.

#### Scenario: Playback continues during inference
- **WHEN** either separator generates artifacts in a background worker
- **THEN** the callback mixes only prepared available audio data
- **AND** model loading and inference do not execute in the callback
