## MODIFIED Requirements

### Requirement: Shared Preprocessing Before Parallel Pipelines
The system SHALL reuse the loader's immutable decoded PCM, convert it once to shared mono
audio and derive each analysis branch's required sample rate without re-decoding the file.

The KeyNet branch SHALL receive mono 44100-Hz input with its existing CQT parameters. The
selected Beat This branch SHALL receive mono 22050-Hz input derived directly from the same
shared mono source. Required-rate conversion SHALL preserve source origin and complete tails;
it SHALL be skipped when the shared input already has that branch's required rate. The key
branch SHALL NOT consume Beat This's downsampled input, and the beat branch SHALL NOT require
an intermediate key-input conversion. Shared inputs SHALL carry explicit rate/source identity.

#### Scenario: A 48000-Hz loaded source feeds both branches
- **GIVEN** immutable stereo loaded PCM at 48000 Hz
- **WHEN** analysis prepares its inputs
- **THEN** one mono conversion supplies both branches
- **AND** the key branch derives 44100-Hz mono and the beat branch derives 22050-Hz mono
- **AND** both retain the same source-time origin without another file decode

#### Scenario: Native key rate does not bypass beat conversion
- **GIVEN** loaded mono PCM at 44100 Hz
- **WHEN** analysis prepares its inputs
- **THEN** KeyNet uses that input without rate conversion
- **AND** Beat This derives its required 22050-Hz input
- **AND** the two pipelines do not share a falsely labeled common-rate buffer

### Requirement: Parallel BPM and Key Detection
The system SHALL execute available beat/BPM and key detection branches concurrently outside
the real-time audio callback after their shared mono source is available. KeyNet SHALL remain
on a Rust background thread without Python/GIL access; Beat This SHALL run through its lazy
isolated background-worker adapter under the same analysis request lifecycle.

The system SHALL assemble the request's result after both branches reach a terminal state,
including explicit unavailable/failure/cancellation. It SHALL retain independent component
statuses rather than failing successful key detection because the optional beat worker is
missing. Queueing, preparation and process overhead SHALL be measured separately; concurrent
execution SHALL NOT be represented as a guarantee that wall time equals one kernel's runtime.

The system SHALL invalidate cancelled key publication immediately, cancel queued key work and
retain ownership of any noninterruptible in-flight native key call until it actually returns.
It SHALL bound active/retiring key slots and retained PCM bytes, applying backpressure when full.
Beat-worker termination SHALL NOT be reported as completed whole-request cancellation while a
key branch remains active. Whole-request terminal status SHALL require both branches to settle;
bounded beat-process termination SHALL NOT imply a hard deadline for native key cancellation.

#### Scenario: Both available branches execute concurrently
- **GIVEN** shared mono PCM and an available selected beat worker
- **WHEN** an analysis request runs
- **THEN** Rust key detection and the isolated beat job can execute concurrently
- **AND** result assembly waits for terminal states without blocking the callback or UI

#### Scenario: Result combines successful component outputs
- **GIVEN** beat and key branches have succeeded for the same source/request
- **WHEN** the result is assembled
- **THEN** it contains beat-derived BPM/grid and the musical key string
- **AND** each component retains its provenance and success status

#### Scenario: Unavailable beat worker does not suppress KeyNet
- **GIVEN** the selected optional beat worker is absent
- **WHEN** a valid source is analyzed
- **THEN** the beat component settles as unavailable
- **AND** Rust KeyNet runs and can publish a successful musical key result
- **AND** no alternate beat detector is silently invoked

#### Scenario: A cancelled native key call is still retiring
- **GIVEN** an in-flight KeyNet call cannot be interrupted immediately
- **WHEN** its request is cancelled and the beat branch has terminated
- **THEN** key work remains retiring with its bounded slot and PCM ownership retained
- **AND** further work observes the configured limits instead of spawning unlimited replacements
- **AND** whole-request cancellation completes only after the key call has actually ended
