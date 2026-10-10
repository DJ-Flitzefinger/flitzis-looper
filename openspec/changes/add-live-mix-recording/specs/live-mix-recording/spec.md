## ADDED Requirements

### Requirement: RECORD uses shared performance controls and effective state (E11-11)
The system SHALL provide a button labeled `RECORD` to the left of global
START/STOP with a deliberate gap, reusing suitable existing button/interaction
helpers and the authoritative global playback implementation (E11-11).

The control SHALL expose Idle, Preparing, Recording, Finalizing and Failed state
truthfully from effective native capture and worker completion. One gesture SHALL
dispatch one action; holding a mouse button SHALL NOT repeatedly toggle a take.
The gap SHALL be at least the existing control-group gap, and scaled/narrow
layout SHALL retain separate nonoverlapping hit targets and complete labels.

#### Scenario: RECORD layout and gesture are unambiguous
- **WHEN** the bottom bar renders at ordinary or narrow supported sizes/scales
- **THEN** RECORD appears left of START/STOP with the required gap and shared button style/helper
- **WHEN** the performer presses and holds RECORD once
- **THEN** exactly one recording intent is dispatched
- **AND** pending or rejected work is not displayed as effective recording

### Requirement: Right RECORD toggles capture without playback changes (E11-12)
The system SHALL start a prepared capture on a right-click press edge from Idle
and request capture stop/drain/finalization on a right-click press edge while
Recording, without starting, stopping, seeking or restoring any pad (E11-12).

Preparing/Finalizing SHALL report busy without admitting another take. Failed
state SHALL retain a visible error until acknowledged; subsequent explicit
gesture may prepare a new take. An empty playback session MAY record silence;
a take with zero captured frames SHALL be reported empty and SHALL NOT publish a
successful recording file. Capture readiness/start/stop failures SHALL be visible.

#### Scenario: Free capture start and stop preserve pad playback
- **GIVEN** pads are playing, paused or stopped
- **WHEN** the performer right-clicks RECORD to start then later to stop
- **THEN** only recording state changes at acknowledged native frame boundaries
- **AND** existing pad positions, playback/paused state and remembered group are unchanged
- **AND** Finalizing remains visible until actual closed file success or error

#### Scenario: Repetition busy error and empty capture are explicit
- **WHEN** a RECORD button is held, clicked while Preparing/Finalizing, or a take fails
- **THEN** no duplicate take or unintended playback action occurs and busy/error is visible
- **WHEN** a right capture stops before recording any frame
- **THEN** empty capture is reported without a successful empty file

### Requirement: Left RECORD atomically captures the existing playback group (E11-13)
The system SHALL make an Idle left-click RECORD prepare the selected-format
writer and atomically start capture plus the existing active/remembered global
START/STOP group from each pad's effective loop start at one Rust-owned start
output frame (E11-13).

The first captured frame SHALL equal that native group start frame. The group
SHALL use the authoritative current source/timing/loop/restore bindings,
residency readiness, scheduler and native ACK, work with MULTI LOOP ON or OFF,
and preserve existing prepared stem masks/rates/DSP intent. Writer-ready permit,
capture/session/feedback/queue/retirement and unchanged existing voice capacity
SHALL be reserved before any effect. Stale intent, unavailable readiness or
resource failure SHALL start neither a partial group nor a take, SHALL capture
no frames and SHALL retire any empty prepared spool through the worker. Empty left
group SHALL report no active/remembered group without capturing.

A left click while Recording SHALL request capture stop/finalization only;
Preparing/Finalizing SHALL report busy. Neither SHALL start a second take or
restart pads. Normal START/STOP SHALL remain independent and SHALL NOT implicitly
stop/finalize recording. Simultaneous left/right press SHALL dispatch only the
right recording action.

#### Scenario: Remembered stem group launches at the first captured frame
- **GIVEN** START/STOP remembers two prepared pads with different effective loop starts and stem masks
- **AND** MULTI LOOP is either ON or OFF and the writer is ready
- **WHEN** the performer left-clicks RECORD
- **THEN** native execution starts both pads from their effective starts at one output frame
- **AND** capture begins at that same frame without prelaunch callback samples
- **AND** their configured stem combinations and existing voice-capacity contract remain intact

#### Scenario: Current group readiness or stale binding rejects the whole effect
- **GIVEN** left RECORD is preparing the current active-minus-paused group
- **WHEN** source/loop/group intent changes or writer/readiness/queue/voice admission fails
- **THEN** neither a partial group restart nor recording start takes effect
- **AND** the visible rejection preserves previous playback and creates no successful partial take

#### Scenario: Repeat left and standard START/STOP remain bounded
- **GIVEN** a take is Recording or Finalizing
- **WHEN** the performer left-clicks RECORD again
- **THEN** Recording requests capture stop or Finalizing reports busy, without another group launch
- **WHEN** the performer uses standard START/STOP
- **THEN** its existing playback behavior applies independently of capture finalization

### Requirement: Recording formats quality and readiness persist explicitly (E11-14)
The system SHALL persist global default WAV, FLAC or MP3 recording format and
per-format quality choices, defaulting to WAV float32, FLAC 24-bit lossless level
8 and MP3 CBR 320 kbit/s as the highest sensible supported defaults (E11-14).

Supported choices SHALL distinguish WAV PCM16/PCM24/float32, FLAC 16/24-bit
precision and compression 0..8, and MP3 CBR 128/192/256/320 kbit/s. FLAC
compression SHALL NOT be presented as audio fidelity. Actual selected codec,
quality, engine output rate/channels and frame duration SHALL be verified after
file close. Unsupported rate/channels/encoder SHALL fail readiness visibly
before launch, without silent fallback, resampling or normalization. Invalid or
missing recording fields SHALL restore explicit recording defaults with a
diagnostic, preserving unrelated project/pad intent.

Float32 capture SHALL preserve exact mixed values. Integer/lossy conversion SHALL
use documented fixed full-scale quantization/saturation and expose peak/clipping
warnings without per-take attenuation/normalization. MP3 delay/padding/trim
metadata SHALL be verified separately from the exact float capture extent.
Long WAV SHALL use verified RF64 or an equivalent large-WAV container without
an artificial RIFF-size/duration cap.

#### Scenario: Real encoded files match persisted choices
- **GIVEN** valid saved WAV, FLAC and MP3 quality settings
- **WHEN** actual captured samples are encoded and the files are reopened independently
- **THEN** actual codec, precision/bitrate, rate, channels and duration match the selected supported choices
- **AND** float capture bounds and MP3 delay/padding handling are verified without claiming lossy PCM identity
- **AND** overrange conversion warnings and absence of normalization are testable

#### Scenario: Encoder invalid settings and long WAV are truthful
- **WHEN** recording settings are malformed or the selected encoder/output geometry is unavailable
- **THEN** recording defaults/diagnostic or readiness failure applies without resetting pads or starting a partial group
- **WHEN** a WAV take crosses the RIFF size boundary
- **THEN** verified RF64/large-WAV publication preserves full frame extent with bounded RAM

### Requirement: Capture is frame exact bounded and realtime safe (E11-15)
The system SHALL capture the final float output mix over exact native
`[first_output_frame, end_output_frame)` extents through a preallocated bounded
RT data queue into a non-RT writer/encoder (E11-15).

Capture SHALL include existing full/stem selection, rate/KEYLOCK, gain/DSP,
velocity, Master Volume and momentary mute before device-format conversion.
In-buffer scheduled start/stop SHALL split the capture extent correctly rather
than include the entire surrounding callback. Duration SHALL grow disk storage,
not a full-take RAM array. The callback SHALL NOT allocate, perform I/O/encoding,
block/lock, access Python/GIL, log or perform unbounded work. Queue overflow,
nonfinite audio, disk/writer/stream failure SHALL visibly fail capture while
playback continues; missing/gapped frames SHALL NOT silently publish success.
Actual queue/held-block memory peaks SHALL be bounded and measured.

#### Scenario: Segmented native capture excludes prestart and poststop samples
- **GIVEN** irregular callback sizes with capture start/stop scheduled inside callbacks
- **WHEN** the productive mixer renders known nonneutral full/stem/DSP/master output
- **THEN** the contiguous spool equals the independent mixed PCM oracle over the exact first/end frames
- **AND** no sample before start or at/after exclusive end is captured
- **AND** callback code/allocation checks and actual storage peaks establish the fixed bound

#### Scenario: Overflow or writer failure cannot hide dropped frames
- **GIVEN** a stalled writer, full capture queue or failing disk/stream
- **WHEN** capture cannot preserve the next contiguous extent
- **THEN** the session becomes Failed with its exact valid-prefix/terminal extent visible
- **AND** playback continues without callback blocking
- **AND** no successful gapped recording is published

### Requirement: Record files drain finalize and recover without overwrite (E11-15)
The system SHALL store recordings under the repository-root `record/` directory,
ignore `/record/` in Git, and publish only a uniquely named closed verified file
after stop acknowledgement, queue drain and encoding finish (E11-15).

Prepared/failing/interrupted takes SHALL have unique recorder-owned incomplete
spool/metadata and SHALL NOT overwrite existing files. Recovery SHALL distinguish
incomplete data from success and preserve unknown files. Shutdown SHALL stop
capture, quiesce/acknowledge its terminal extent, drain and finalize before audio
teardown; failure or bounded shutdown deadline SHALL retain recoverable data and
a visible diagnostic. Finalizing SHALL not admit a second take.

#### Scenario: Stop succeeds only after real drain and close
- **GIVEN** captured blocks remain when stop is acknowledged
- **WHEN** the worker drains, closes, encodes and independently verifies the take
- **THEN** a unique recording is atomically published only after those operations succeed
- **AND** the controller shows Finalizing until success or failure is known

#### Scenario: Shutdown reopen and filename collision preserve evidence
- **GIVEN** a take is active, finalizing or interrupted and a target name already exists
- **WHEN** shutdown, recovery or publication runs
- **THEN** terminal data is drained/finalized or retained as explicitly incomplete
- **AND** existing/unknown files are not overwritten/deleted or reported as the new completed take
- **AND** outputs under /record/ remain ignored by Git
