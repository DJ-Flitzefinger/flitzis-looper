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

For diagnostic analysis, the shared mono source SHALL be a complete staged loaded-rate file.
Key preparation SHALL read it through the retained native handle in bounded chunks and produce
the complete 44100-Hz vector for the unchanged CQT/KeyNet implementation. Conversion SHALL use
the existing Rubato FFT configuration, preserve time zero, remove leading algorithmic delay
exactly once, flush the complete tail and return `ceil(source_frames * 44100 / loaded_rate)`
frames. At 44100 Hz the staged f32 samples SHALL be read exactly without rate conversion.
Chunking SHALL NOT introduce per-chunk origins, shortened inference, alternate key parameters
or a second full loaded-rate PCM allocation.
Tail flushing in standard analysis preparation and diagnostic key preparation SHALL allow
valid zero-output padding calls within a shared finite bound derived from source/target converter
dimensions and the remaining required output. Both paths SHALL check cancellation between calls
and report a conversion failure if that bound cannot produce the complete tail. Standard
conversion SHALL preserve its existing PCM allocation limits, source origin, single leading-delay
trim and complete ceiling output length.

#### Scenario: Mono buffer is shared between pipelines
- **GIVEN** immutable stereo loaded PCM at 48000 Hz
- **WHEN** analysis prepares its inputs
- **THEN** one mono conversion supplies both branches
- **AND** the key branch derives 44100-Hz mono and the beat branch derives 22050-Hz mono
- **AND** both retain the same source-time origin without another file decode

#### Scenario: Mono buffer at native 44100 Hz skips resampling
- **GIVEN** loaded mono PCM at 44100 Hz
- **WHEN** analysis prepares its inputs
- **THEN** KeyNet uses that input without rate conversion
- **AND** Beat This derives its required 22050-Hz input
- **AND** the two pipelines do not share a falsely labeled common-rate buffer

#### Scenario: Staged key input matches the complete prior conversion
- **GIVEN** complete loaded-rate mono at 22050, 44100, 48000 or 96000 Hz with leading silence,
  first/last impulses and a fractional resampled frame count
- **WHEN** the native key branch reads and converts bounded chunks of the staged source
- **THEN** the complete key vector preserves the prior mono/converter output and source origin
- **AND** its length uses the ceiling rule with delay removed once and the tail retained
- **AND** the unchanged full CQT/KeyNet path receives the same complete signal

#### Scenario: A buffered FFT tail initially produces no output
- **GIVEN** a valid 96000-Hz input ends with a fractional FFT/input-block remainder
- **AND** the first zero-padding call produces no output while the converter accumulates its FFT unit
- **WHEN** standard analysis preparation or diagnostic key preparation flushes the tail
- **THEN** it continues within the calculated finite call bound to obtain the complete output
- **AND** source origin, ceiling frame count and existing FFT sample values remain preserved
- **AND** the output matches an independent full-block conversion with explicit zero padding
  after one delay trim and retention of the original ceiling frame count
- **AND** a cancelled or exhausted flush fails explicitly rather than looping without a bound

#### Scenario: A failed staged key read preserves independent beat output
- **GIVEN** beat analysis succeeds but native key preparation encounters an invalid or
  incomplete staged source read
- **WHEN** both branches settle and the request is still current
- **THEN** the key outcome explicitly fails with `unknown` while valid beat output survives
- **AND** no shortened key vector is reported as a complete successful analysis

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

#### Scenario: Both pipelines run concurrently
- **GIVEN** shared mono PCM and an available selected beat worker
- **WHEN** an analysis request runs
- **THEN** Rust key detection and the isolated beat job can execute concurrently
- **AND** result assembly waits for terminal states without blocking the callback or UI

#### Scenario: Analysis result combines both pipeline outputs
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
