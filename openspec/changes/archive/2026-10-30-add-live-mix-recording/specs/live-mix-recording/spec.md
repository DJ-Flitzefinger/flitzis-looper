## ADDED Requirements

### Requirement: Portable activation verifies current recording interfaces
Portable activation SHALL verify the recording interfaces before constructing
the application or opening audio devices. Start and stop SHALL expose an optional
`target_frame=None` for Native next-admissible timing. GLOBAL publication SHALL
retain its required original captured group and keyword-only `capture=None`
attachment. Preflight SHALL reject incompatible older signatures without executing
capture, playback, recovery inspection or file publication.
Portable preflight and ordinary developer starters SHALL also require the original
Native stream-failure observation and checked failed-stream-quiescence methods
before application activation. They SHALL verify those methods without invoking
them or opening an engine or device.

#### Scenario: Stale recording signatures cannot activate a current checkout
- **GIVEN** a Native module has capture methods with required target frames or lacks the keyword-only GLOBAL capture attachment
- **WHEN** Portable preflight checks the module
- **THEN** activation fails before application, device or recording work
- **AND** a compatible module is verified only by reflection

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

Free capture start/stop SHALL use a Native next-admissible target, without a
Python clock estimate or arbitrary one-frame offset. Native SHALL resolve that
request once to the admitting callback's first unrendered frame after all existing
admission/deferral gates. Unadmitted deferred intent SHALL remain unresolved.
Requested mode/explicit target SHALL remain distinct from effective frame ACKs.
Explicit frame requests SHALL retain their existing past/start/stop semantics;
repeated identical stop intent SHALL NOT enqueue or retarget another stop.

#### Scenario: Free capture resolves native time after actual admission
- **GIVEN** the callback clock is nonzero and free capture intent is deferred
- **WHEN** the original request becomes admissible in a later callback
- **THEN** Native resolves its next-admissible target to that callback's first unrendered frame
- **AND** exact captured samples follow the actual first/end ACKs without a Python time estimate
- **AND** full-queue retries and duplicate stop gestures preserve the original session and stop intent

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

Capture start authorization SHALL have one atomic decision ordered against
control/worker failure. In the combined group commit it SHALL be the last
fallible claim after all member claims; rejection SHALL abort every claimed
member before any effect. Once that claim succeeds, only infallible effect
publication SHALL follow. Later capture failure SHALL remain visible without
rolling back already accepted playback. A start permission claim SHALL NOT
itself report effective capture started or a first-frame ACK.
The permission SHALL remain bound to its original live capture owner until
consuming publication and SHALL prevent foreign, repeated or intervening-mutated
owner publication without adding a fallible check after combined effects.
The combined group SHALL use one existing scheduled group action and reserve its
own stop capacity. Replanning SHALL preserve its original input order and any
already admitted stop endpoint. A stop before a new proposed execution frame
SHALL reject the whole unstarted group and empty take; equality SHALL execute
group start before its own stop and acknowledge an empty capture. An uncommitted
group SHALL NOT silently consume and lose an executed stop. All settlement SHALL
match the original capture and group; duplicates and unrelated events SHALL NOT
cancel a previously admitted take or overwrite an accepted receipt.

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

The selected format and every per-format quality choice SHALL form one validated
immutable options value. Preparing a take SHALL retain that value; subsequent
Settings edits SHALL affect future takes without changing the prepared take.
Recording-only recovery diagnostics SHALL NOT create or clear an unrelated
project-save, stem or migration safety fence.

Float32 capture SHALL preserve exact mixed values. Integer/lossy conversion SHALL
use documented fixed full-scale quantization/saturation and expose peak/clipping
warnings without per-take attenuation/normalization. MP3 delay/padding/trim
metadata SHALL be verified separately from the exact float capture extent.
Long WAV SHALL use verified RF64 or an equivalent large-WAV container without
an artificial RIFF-size/duration cap.

Selected-codec readiness SHALL include an actual finite encode/reopen roundtrip
at the retained engine output geometry. Integer precision B SHALL use finite
input clamped to [-1, 1], scaled by 2^(B-1), rounded ties to even and clamped to
the signed B-bit range, without dither. MP3 SHALL receive fixed 24-bit PCM.
Original abs(sample)>1 sample/frame counts SHALL be distinct from endpoint
quantization; float32 WAV SHALL retain payload bits including signed zero and
overrange. Lossless output SHALL match the complete selected PCM payload, while
MP3 SHALL verify per-packet constant bitrate, contiguous timing, first/last trim
and full decoded frame extent. Conversion and verification SHALL use fixed-size
blocks or streamed packet records rather than take-sized arrays.

#### Scenario: Legacy or partially malformed recording settings preserve the project
- **GIVEN** a saved project has valid pad, key, loop and gain intent but missing or malformed recording fields
- **WHEN** the project is restored
- **THEN** only missing or invalid recording choices use their explicit defaults and valid sibling format choices remain intact
- **AND** Settings and existing console diagnostics expose a recording-settings notice without blocking ordinary project saves
- **AND** an independently invalid stem or migration field retains its existing save fence

#### Scenario: Settings replacement cannot mutate a prepared take
- **GIVEN** a take retained validated WAV, FLAC and MP3 choices when preparation began
- **WHEN** the performer changes recording defaults in Settings
- **THEN** the prepared take retains its original options and future takes use the new validated value
- **AND** editing these defaults does not change playback, rate, pitch or a pad's source

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

#### Scenario: Selected codec cannot silently change geometry or retain untrimmed MP3
- **GIVEN** the selected MP3 encoder cannot represent the current output rate or channel count
- **WHEN** the actual preparation roundtrip executes
- **THEN** readiness fails before capture or group admission without downmixing, rate substitution or format fallback
- **GIVEN** an encoded MP3 lacks valid first/last trim or contains variable packet bitrate despite a nominal constant-bitrate tag
- **WHEN** final file verification executes
- **THEN** no successful recording is published from container bitrate or duration tags alone

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

The existing off-realtime writer SHALL bind an immutable SHA256 witness to the
actual complete payload bytes it wrote while retaining exclusive writer custody.
The original live successful terminal session SHALL provide that witness;
finalization SHALL compare its complete finite source-payload scan before encoding.
FileID, valid headers and matching frame extents alone SHALL NOT certify unchanged
writer bytes after close. No payload hashing SHALL run on the audio callback.

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

#### Scenario: Final writer decision has one cancellation winner
- **GIVEN** the RT extent is sealed and the writer has drained and synced its payload
- **WHEN** cancellation or stream failure races the writer's terminal decision
- **THEN** one atomic decision SHALL determine whether that control failure was accepted
- **AND** a later cancellation SHALL report refusal without retroactively changing that decision
- **AND** subsequent writer or metadata I/O failure SHALL still fail finalization and remain visible

#### Scenario: Failure and start permission have one ordering
- **GIVEN** the original prepared capture has not published an effective start
- **WHEN** control or worker failure wins before its atomic start permission claim
- **THEN** the claim rejects without publishing started or a first-frame ACK
- **WHEN** the start claim wins before a later failure
- **THEN** that failure remains authoritative while the claimed infallible start publication completes
- **AND** the private permission flag never becomes a recovery-header failure code or an effective ACK by itself

#### Scenario: Recovery checkpoint never certifies successful publication
- **GIVEN** a final metadata write leaves a checksum-valid sealed candidate but its later sync or seek reports failure
- **WHEN** the current session or a later process inspects the spool
- **THEN** the current session SHALL retain its writer failure and no successful recording SHALL be published from the checkpoint alone
- **AND** inspection SHALL expose only an unverified recovery candidate, requiring independent payload and selected-format verification before any completed-file claim
- **AND** unavailable storage SHALL not be assumed capable of persisting its own failure diagnostic

#### Scenario: In-place finite payload edits cannot borrow the successful writer session
- **GIVEN** a real writer has closed successfully and its original live ticket retains the writer payload witness
- **WHEN** finite payload bytes change in place before finalizer pin acquisition while FileID, headers and frame extents remain unchanged
- **THEN** full source verification rejects the altered bytes before encoding or successful publication
- **AND** the original live writer witness and unknown files remain unchanged

### Requirement: Record files drain finalize and recover without overwrite (E11-15)
The system SHALL store recordings under the repository-root `record/` directory,
ignore `/record/` in Git, and publish only a uniquely named closed verified file
after stop acknowledgement, queue drain and encoding finish (E11-15).

Prepared/failing/interrupted takes SHALL have unique recorder-owned incomplete
spool/metadata and SHALL NOT overwrite existing files. Recovery SHALL distinguish
incomplete data from success and preserve unknown files. Shutdown SHALL stop
capture, quiesce/acknowledge its terminal extent, drain and finalize before audio
teardown; failure or bounded shutdown deadline SHALL retain recoverable data and
an actual console diagnostic. Finalizing SHALL not admit a second take.
New-take admission SHALL wait for actual worker return and join and release of
all outer readiness, file, namespace and exception-held custody, in addition to
Native terminal settlement. Private component-retired events SHALL NOT substitute
for this boundary. Failed with unresolved custody SHALL remain busy after error
acknowledgement. Shutdown polling SHALL not be starved by unrelated pending work;
a deadline SHALL not fabricate retirement or callback quiescence.

The App SHALL poll the sole recording controller through its always-reachable
runtime tick, including Settings and closing. Its first close request SHALL latch
one shutdown deadline; subsequent frames SHALL NOT extend that deadline. Recording
settlement SHALL be serviced independently of unrelated pending arrangement work,
before source assignments or audio ownership are released.
Technical recording failure details SHALL be reported through the existing
deduplicated control-thread console diagnostics, including without GUI rendering.
The recording surface SHALL retain truthful phase, effective/busy state and
acknowledgement controls without displaying raw technical errors in labels or tooltips.

Failed-stream coordination SHALL observe the original Native stream failure
lifecycle independently of the capture's sticky first failure. An earlier capture
cancellation or overflow SHALL NOT hide a later actual stream failure. A positive
quiescence receipt SHALL require actual stream closure and a checked join of the
existing callback-image retirement worker. A failed or uncertain join SHALL remain
uncertain on repeated calls; a missing stream handle alone SHALL NOT grant orphan
takeover or retirement. Engine, original capture ticket and writer SHALL remain
available for their actual drain, close and join. This receipt SHALL NOT substitute
for recording-worker return, custody release or file verification.
Once the original stream failure is positively observed, effective recording
SHALL remain false for that take even while an older worker snapshot still
reports an unsealed active capture, both before and after checked quiescence.

#### Scenario: A prior capture error cannot hide a dead callback owner
- **GIVEN** the original capture already reports cancellation or overflow and its original stream subsequently fails
- **WHEN** the App services recording without another audio callback
- **THEN** it observes the independent original stream failure and requests actual checked callback quiescence
- **AND** only a verified close barrier permits the existing original orphan-publication settlement
- **AND** the earlier capture error remains unchanged, with no fabricated start or retirement ACK
- **AND** failed checked closure remains visibly unresolved on retry

#### Scenario: Observed stream death overrides an older active snapshot
- **GIVEN** the original worker snapshot reports a started, unsealed capture without a capture failure
- **WHEN** the independent original stream lifecycle positively reports failure
- **THEN** the RECORD surface ceases to report effective recording immediately
- **AND** older snapshots cannot restore that indication before or after checked callback closure
- **AND** recording remains busy until its actual worker and custody owners retire

#### Scenario: Closing services recording with one fixed deadline
- **GIVEN** recording finalization and unrelated arrangement work are pending
- **WHEN** the App receives a close request and continues frame polling
- **THEN** recording settlement is serviced on every close frame with the original latched deadline
- **AND** unrelated pending work cannot starve recording or cause early audio teardown
- **AND** the close surface retains compact pending progress while actual recording errors and retained incomplete evidence are reported through the existing deduplicated console diagnostics

The successful non-replacing final-link operation SHALL be the publication
linearization point. Required file/namespace/codec checks SHALL precede it.
Subsequent custody-release failure SHALL retain an explicit committed file and
verified take outcome with independent uncertain retirement; it SHALL NOT become
an unpublished-take failure, delete that file or fabricate completed retirement.
An actual custody-release error before publication SHALL likewise keep retirement
explicitly uncertain and SHALL NOT authorize a replacement owner.

Startup recovery discovery SHALL stream only immediate recording-attempt entries
with bounded metadata reads and cancellable iteration. It SHALL preserve unknown,
malformed or conflicting data. Declared options, canonical metadata and observed
FileIDs SHALL establish at most present consistency, never authenticated original
capture provenance, successful retirement or live Native publication permission.
An observed same-file final link SHALL remain unverified until independent content
inspection. Discovery SHALL perform no encoding, publication, deletion or repair.
The original ordinary root and ancestors SHALL remain pinned through scanner
ownership; per-attempt pins SHALL close before each observation is yielded.

#### Scenario: Restart discovery cannot reconstruct live recording permission
- **GIVEN** an interrupted attempt or a final link and unsigned recording metadata remain after restart
- **WHEN** startup discovers immediate recording-attempt entries
- **THEN** bounded observations distinguish incomplete candidates, same-file final links and conflicts
- **AND** valid-looking or in-place changed metadata grants no original ticket, completed take or completed retirement
- **AND** cancellation releases the scanner and owned entry pins without modifying unknown data

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

#### Scenario: Post-commit failures preserve the published outcome
- **GIVEN** the verified encoded file is atomically linked to its unique final name
- **WHEN** a later namespace read or custody release would report an error
- **THEN** no post-link validation changes the successful commit into a missing-file failure
- **AND** a custody-release error retains the committed path, identity and verified completion while unresolved retirement remains explicit

#### Scenario: Pre-commit custody failure cannot certify retirement
- **GIVEN** finalization rejects a take before the final link is created
- **WHEN** closing an original namespace pin reports an error
- **THEN** no final file is reported and existing incomplete evidence is retained
- **AND** retirement remains explicitly unproven instead of allowing another owner
