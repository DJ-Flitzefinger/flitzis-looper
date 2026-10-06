## ADDED Requirements

### Requirement: Explicit Native Acceptance Uses Actual Loaded Pad Evidence
The system SHALL capture actual loaded source/PCM/timebase/generation/request in
an opaque ticket and execute lossless QM preparation outside realtime processing.
It SHALL freshly construct acceptance with explicit independent quarter counts,
timing-error, origin and acceptance assertions, without inferring these from fit
quality or promoting existing timing numbers.

#### Scenario: The performer has not supplied independent musical acceptance
- **GIVEN** an ordinary loaded pad with legacy BPM or a prepared QM raw sequence
- **WHEN** no independent count and named acceptance assertion is supplied
- **THEN** no accepted timing record is published
- **AND** ordinary loading and manual/TAP behavior remain available

#### Scenario: Actual loaded PCM differs from a caller snapshot
- **GIVEN** a caller retains matching metadata for an earlier source
- **WHEN** the native source has been replaced before preparation or publication
- **THEN** native current ownership controls validity
- **AND** the caller snapshot cannot authorize the earlier result

### Requirement: Native Timing Adoption Rejects Retired Ownership
The system SHALL connect TimingAdoptionGuard to actual native source/request/
intent and recheck ownership after preparation, at enqueue and mixer adoption.
Requests, source changes, unload, cancellation and timing edits SHALL revoke
pending work. Manual/Tap/Legacy SHALL block automatic adoption; returning to
Automatic SHALL NOT revive tickets. Failure/full queues SHALL preserve effective
state and counters SHALL reject wrap.

#### Scenario: A timing edit occurs while preparation runs
- **GIVEN** accepted-timing preparation for an actual loaded pad
- **WHEN** a manual, TAP, legacy or same-value timing edit supersedes its intent
- **THEN** the prepared result cannot publish
- **AND** the previously effective audio state is retained

#### Scenario: A newer request wins after enqueue
- **GIVEN** a valid pending accepted-timing publication
- **WHEN** a new request or source change occurs before mixer adoption
- **THEN** the callback rejects the pending publication using bounded checks
- **AND** acknowledgement reports rejection rather than availability

#### Scenario: A legacy result was queued before a new accepted request
- **GIVEN** a completed normal analysis or current-source load with retired timing
- **WHEN** native polling delivers its terminal event after a newer request
- **THEN** matching source/task bookkeeping can settle without replaying legacy timing
- **AND** automatic grid/loop initialization and restore replay cannot revoke the newer timing
- **AND** replaced-source events are skipped without stopping the remaining event drain

#### Scenario: Publication cannot reserve queue capacity
- **GIVEN** a valid current preparation ticket and previously effective timing
- **WHEN** the command ring is full or a required identity cannot advance
- **THEN** the publication fails without changing effective timing or wrapping identities

### Requirement: Native SourceGrid Consumes Acknowledged Exact Accepted Timing
The system SHALL publish fixed full-revision/binary64-period/signed-origin
metadata and use the period directly in live SourceGrid after mixer acceptance.
It SHALL distinguish pending from accepted, prevent older legacy updates from
overriding later acceptance, retire large owners outside the callback, and
preserve physical endpoints, source zero and active source progression.

#### Scenario: Fractional timing cannot be represented through binary32 BPM
- **GIVEN** explicitly accepted source-bound timing with a fractional binary64 period
- **WHEN** the mixer adopts it and evaluates the native source grid
- **THEN** the grid uses the accepted period and signed origin directly
- **AND** complete accepted revision identity is retained even for equal-valued records

#### Scenario: Legacy metadata was queued before precise publication
- **GIVEN** an older BPM or origin update awaiting callback application
- **WHEN** a later accepted publication becomes effective
- **THEN** the older update cannot replace its accepted period or origin
- **AND** a successful later explicit timing edit revokes accepted authority

#### Scenario: A publication has only reached the command ring
- **GIVEN** a current accepted record enqueued for callback adoption
- **WHEN** the callback has not yet accepted it
- **THEN** feedback reports pending status
- **AND** it does not report the new timing as effective

### Requirement: Current Accepted Timing Is Native Acknowledged Authority
The system SHALL resolve current accepted timing from actual native loaded-source
ownership and callback acknowledgement, retaining the complete accepted revision,
exact period and signed origin. Historical ticket feedback SHALL NOT substitute
for current pad authority. Pending replacement SHALL leave the previous effective
record observable; successful source or timing-intent replacement SHALL revoke it.

#### Scenario: An accepted ticket becomes historical
- **GIVEN** a callback-acknowledged accepted record for a loaded pad
- **WHEN** the source is unloaded or replaced, or a Manual, Tap or Legacy edit succeeds
- **THEN** current-pad resolution no longer exposes that record
- **AND** historical ticket feedback cannot revive it

#### Scenario: A newer automatic record is still pending
- **GIVEN** a current acknowledged accepted record and a valid replacement
- **WHEN** the replacement is enqueued but has not been acknowledged
- **THEN** current resolution retains the previous effective revision
- **AND** it exposes the replacement only after actual callback adoption

### Requirement: Native Period And Rate Consumers Preserve Binary64 Timing
The system SHALL use acknowledged accepted seconds-per-quarter directly for
native source timing, transport anchoring/bootstrap, output-clock quantization
and BPMLOCK, converting legacy public BPM only at admission without a binary32
roundtrip. It SHALL derive the playback target once as source period divided by
master output period, preserving binary64 rates and fractional source epochs.

#### Scenario: Accepted timing differs from a binary32 BPM projection
- **GIVEN** an acknowledged nonintegral binary64 source period and master output period
- **WHEN** production bootstrap, quantization and BPMLOCK rendering consume them
- **THEN** transport and the shared output clock retain the exact master period
- **AND** SourcePlayback uses one binary64 source-to-output-period ratio
- **AND** callback partitioning does not introduce a second rate or source epoch

#### Scenario: The requested BPMLOCK rate exceeds its supported range
- **GIVEN** a source period and master output period whose ratio requires clipping
- **WHEN** the native renderer applies its supported speed limit
- **THEN** the master period remains the requested authoritative period
- **AND** the clipped physical progression does not claim sustained musical SYNC

### Requirement: Pitch ABI And Physical Wrap Preserve The Shared Trajectory
The system SHALL use explicit Rubber Band pitch conversion at its ABI boundary
without changing the shared source trajectory or existing physical-loop wrap
policy. Rate clipping SHALL NOT redefine the authoritative master period.

#### Scenario: Key Lock follows the shared binary64 source rate
- **GIVEN** an acknowledged period-driven source trajectory
- **WHEN** the native Key Lock adapter receives its inverse-rate pitch scale
- **THEN** the existing native double ABI retains binary64 pitch conversion
- **AND** pitch update thresholds, source epoch and physical wrap policy remain unchanged

### Requirement: Python Consumers Resolve One Current Source Timing Projection
The system SHALL resolve one Python source timing snapshot from manual intent,
current native acknowledgement or Legacy policy, retaining exact period, signed
origin, loaded rate, full revision and provenance. Musical grids, labels and loop
operations SHALL consume its period directly. Automatic intent without
acknowledgement SHALL remain unavailable; historical tickets and saved analysis
SHALL NOT establish current authority.

#### Scenario: Current accepted timing disagrees with saved analysis
- **GIVEN** native current timing with a fractional period and signed origin
- **WHEN** Python renders its grid or calculates musical loop boundaries
- **THEN** one source timing snapshot supplies its period, origin and loaded rate
- **AND** physical endpoints are rounded only at the final loaded-frame boundary
- **AND** equal-valued replacement records retain their different complete revisions

#### Scenario: A current lookup is unavailable during automatic replacement
- **GIVEN** Automatic intent and an unavailable current acknowledgement
- **WHEN** Python reads or refreshes derived timing
- **THEN** current musical timing is unavailable for that operation
- **AND** no old analysis value is replayed as native Legacy timing

#### Scenario: Saved source duration disagrees with current native extent
- **GIVEN** current accepted ownership with loaded rate and full frame extent
- **WHEN** Python bounds ALL, maximum auto-loop length or waveform navigation/data
- **THEN** it uses the actual accepted extent from the same source snapshot
- **AND** source replacement invalidates waveform cache/view identity using current native ownership
- **AND** timing-only revisions of that same source do not reset its navigation

### Requirement: Python Global Controls Preserve Current Period And Timing Authority
The system SHALL derive accepted BPMLOCK master timing directly as current anchor
period divided by global speed and publish its binary64 output period without a
BPM conversion. Restore and derived refresh SHALL preserve Automatic authority;
intentional Manual, Tap and Legacy edits SHALL retire accepted authority under
the existing explicit policy. Failed initial pad-BPM admission SHALL preserve saved BPM
intent and previously effective accepted timing.

#### Scenario: Accepted anchor changes before the next speed operation
- **GIVEN** BPMLOCK and a newly acknowledged current anchor revision
- **WHEN** a speed or displayed-BPM control operation resolves that anchor
- **THEN** it derives the requested rate and master period from current source timing
- **AND** stale session anchor BPM cannot define the accepted master period

#### Scenario: Restore runs with current accepted timing
- **GIVEN** current Automatic acknowledged timing and older saved analysis/grid data
- **WHEN** project state or derived BPM controls are refreshed
- **THEN** legacy pad BPM and origin setters do not revoke accepted timing
- **AND** an explicit saved manual override retains its own authority

#### Scenario: An explicit grid correction leaves accepted authority
- **GIVEN** current accepted timing and legacy analysis/grid intent
- **WHEN** the performer deliberately edits the legacy grid offset
- **THEN** the edit revokes accepted authority and resumes coherent legacy timing
- **AND** it does not cache or persist accepted evidence as a manual override

#### Scenario: Direct master period and compatibility BPM are queued together
- **GIVEN** valid binary64 period and BPM parameter writes
- **WHEN** the callback coalesces the master parameter lane
- **THEN** the last admitted value wins in their shared slot
- **AND** direct period bits reach transport and mixer unchanged
- **AND** invalid or full-queue admission does not change effective master timing

#### Scenario: Accepted locked speed cannot reserve parameter capacity
- **GIVEN** an accepted BPMLOCK anchor and previously effective speed/master period
- **WHEN** its combined speed/master-period parameter cannot be admitted
- **THEN** neither parameter nor saved speed changes
- **AND** successful admission records both values as one bounded callback effect

### Requirement: MIDI Runtime Binds Current Source And Complete Timing Authority
The system SHALL bind productive MIDI runtime publication and direct triggers to
actual native source ownership and complete current acknowledged accepted timing.
Endpoint or BPM equality SHALL NOT substitute for source or full revision identity.
Manual, Tap, Legacy and unavailable Automatic SHALL keep their own authority
without promoting numerical values or historical tickets.

#### Scenario: Current native timing supplies MIDI runtime ownership
- **GIVEN** an actual loaded source with current acknowledged accepted timing
- **WHEN** productive MIDI runtime publication or a direct trigger captures its binding
- **THEN** it binds actual source identity, generation, content digest, loaded rate and full extent
- **AND** it carries complete current accepted revision, exact binary64 period and signed origin

#### Scenario: An equal-valued accepted record replaces the current record
- **GIVEN** two accepted records with equal periods, origins and physical endpoints
- **WHEN** the callback acknowledges the replacement's different complete revision
- **THEN** MIDI runtime publication observes the new current revision
- **AND** a trigger from the earlier runtime cannot apply its old derived loop intent

#### Scenario: A source is replaced at the same path and shape
- **GIVEN** a MIDI runtime snapshot for a loaded source
- **WHEN** the current source generation or content changes with equal path, rate and extent
- **THEN** the old snapshot cannot authorize a trigger for the replacement
- **AND** a fresh source-bound runtime publication is required

#### Scenario: Automatic timing is unavailable or explicitly retired
- **GIVEN** Automatic timing without current acknowledgement or a successful Manual, Tap or Legacy edit
- **WHEN** MIDI runtime refresh or a historical runtime trigger occurs
- **THEN** historical accepted timing cannot restore retired authority or its derived loop region
- **AND** fresh Manual, Tap and Legacy states remain supported under their own authority

### Requirement: MIDI Trigger Adoption Is One Bounded Source-Matching Effect
The system SHALL admit direct MIDI loop/playback as one guarded bounded effect and
recheck current source and timing at callback adoption and quantized execution.
Stale execution SHALL apply neither loop nor playback. Failure SHALL preserve
runtime/audio state. Failed-direct fallback SHALL resolve fresh authority and use
the same transaction. Callback work SHALL remain bounded and realtime-safe.

#### Scenario: The callback processes a guarded MIDI trigger
- **GIVEN** an admitted fixed source/timing-bound MIDI trigger
- **WHEN** the callback checks its adoption or scheduled execution
- **THEN** its source and timing checks remain bounded
- **AND** it does not scan or hash PCM, acquire locks, allocate evidence, access Python or perform I/O

#### Scenario: Authority changes after direct MIDI enqueue
- **GIVEN** a source-bound MIDI trigger admitted before a successful timing edit or source replacement
- **WHEN** the callback reaches that trigger after its authority has been retired
- **THEN** neither its loop region nor its playback effect is applied
- **AND** it cannot revive accepted timing from the old runtime snapshot

#### Scenario: Quantized execution outlives the runtime snapshot
- **GIVEN** a bound exclusive MIDI trigger scheduled for a later output frame
- **WHEN** its source, accepted revision, authority or effective runtime loop intent changes before that frame
- **THEN** scheduled execution rechecks the binding before any loop or playback mutation
- **AND** stale execution neither rewrites the current loop nor stops unrelated voices

#### Scenario: A replacement remains pending or is rejected
- **GIVEN** current acknowledged accepted timing and a pending replacement
- **WHEN** MIDI runtime resolves current timing before replacement adoption or after rejection
- **THEN** the previous effective accepted revision remains authoritative
- **AND** enqueue alone never publishes the replacement as current MIDI timing

#### Scenario: Runtime refresh or trigger admission fails
- **GIVEN** a current runtime and previously effective loop/playback state
- **WHEN** a stale or invalid refresh is rejected or the trigger ring is full
- **THEN** no partial loop/play transaction is admitted
- **AND** the previous runtime and audio state are preserved

#### Scenario: Python receives a failed direct MIDI event
- **GIVEN** a direct MIDI trigger could not be admitted
- **WHEN** Python handles its fallback after a source or authority change
- **THEN** fallback resolves fresh current source and timing authority
- **AND** unavailable Automatic timing does not overwrite the native loop
- **AND** fallback admits no partial unguarded loop/play sequence
- **AND** it does not replay the old runtime snapshot

### Requirement: Prepared Sources Capture Current Native Timing Authority
The system SHALL bind productive prepared-source capture and permits to actual
current native source/request/epoch ownership and declared authority. Available
Automatic SHALL retain complete acknowledged accepted revision, exact binary64
period and signed origin; unavailable Automatic SHALL fail admission. Manual, Tap
and Legacy SHALL keep their own authority without promotion to accepted evidence.

#### Scenario: Current Automatic timing supplies prepared-source ownership
- **GIVEN** an actual loaded source with current acknowledged Automatic timing
- **WHEN** a productive prepared-source ticket is captured
- **THEN** its permit binds the actual source and complete current accepted revision
- **AND** it retains the exact binary64 period and independent signed origin rather than a BPM or endpoint projection
- **AND** source/content/request/rate/epoch ownership checks remain effective
- **AND** the shared current resolver verifies source generation, content digest, loaded rate, channels and full extent at capture
- **AND** the fixed callback binding carries source address/shape/rate, monotonic authority revision and full accepted projection with checked request/epoch owners
- **AND** its pinned source and matching source_version preserve ownership without separately retained generation/digest fields in realtime processing

#### Scenario: Automatic has no consistent current acknowledgement
- **GIVEN** Automatic intent without a current source-matching accepted record
- **WHEN** prepared-source capture or publication is attempted
- **THEN** admission fails without replacing previously admitted audio
- **AND** historical accepted ticket metadata or saved numerical timing cannot authorize the set
- **AND** raw revision, endpoint equality, source hash or preparation epoch alone cannot establish accepted authority

#### Scenario: Manual Tap or Legacy prepares stems
- **GIVEN** a current loaded source with declared Manual, Tap or Legacy authority
- **WHEN** its productive stem preparation is captured and admitted
- **THEN** the binding retains that authority and current source ownership
- **AND** equal numerical timing cannot promote it to an accepted record or revive retired Automatic evidence

### Requirement: Prepared Stem Adoption Rechecks Current Source And Timing
The system SHALL recheck productive PreparedStemSet ownership and fixed timing
binding before and after off-thread preparation, through enqueue and at callback
adoption. Stale source, authority or accepted projection SHALL prevent adoption.
Failed admission and rejection SHALL preserve prior audio; pending/rejected timing
SHALL NOT become current evidence. Callback checks SHALL remain bounded and
realtime-safe, with large owners/evidence retired off-thread.

#### Scenario: A current prepared set reaches callback adoption
- **GIVEN** a productive PreparedStemSet and its captured source/request/preparation-epoch owner
- **WHEN** preparation and publication run
- **THEN** source/request/epoch and fixed current timing binding are checked before off-thread decoding/alignment, after preparation, through enqueue and at adoption
- **AND** stale complete accepted revision, exact period or signed origin rejects adoption even if numerical endpoints or PCM shape match
- **AND** callback checks do not acquire locks, scan/hash PCM, allocate evidence, access Python/UI, perform I/O or log
- **AND** large sample owners and evidence retire outside realtime processing

#### Scenario: Accepted revision changes without changing timing numbers
- **GIVEN** a prepared ticket captured under one acknowledged accepted revision
- **WHEN** an equal-period equal-origin replacement with a different complete revision is acknowledged before stem adoption
- **THEN** the old prepared set is rejected
- **AND** endpoint equality and the still-matching source cannot authorize that old set
- **AND** previous full-mix or admitted-stem audio remains available

#### Scenario: A source or timing edit races off-thread preparation
- **GIVEN** current source-bound stem preparation running outside the callback
- **WHEN** source replacement or a successful authority/timing edit occurs before its post-preparation or enqueue check
- **THEN** the prepared result cannot publish under its captured binding
- **AND** previously admitted audio remains unchanged

#### Scenario: Source or timing changes after stem enqueue
- **GIVEN** a valid prepared set pending callback adoption
- **WHEN** its actual source, declared authority or acknowledged accepted projection changes before adoption
- **THEN** callback adoption reports rejection using bounded source/atomic/fixed-projection checks
- **AND** neither the set nor its historical timing replaces existing audio

#### Scenario: An accepted replacement remains pending or is rejected
- **GIVEN** current acknowledged accepted timing and a pending replacement
- **WHEN** prepared-source resolution runs before adoption or after rejection
- **THEN** it resolves the previous effective accepted revision
- **AND** the proposed replacement cannot authorize preparation as current timing
- **AND** existing request/epoch guards still reject preparation jobs retired by a newer request

#### Scenario: The publication ring has no capacity
- **GIVEN** a valid current prepared-source ticket and existing audio
- **WHEN** enqueue cannot reserve capacity
- **THEN** its ticket remains unconsumed and no partial set is admitted
- **AND** previous audio and effective timing remain unchanged

#### Scenario: Control authority retires before the mixer clear executes
- **GIVEN** an admitted Legacy, Manual or Tap edit has revoked control acknowledgement while the parameter callback still holds the old effective accepted projection
- **WHEN** a fresh nonaccepted prepared-source ticket reaches callback adoption during that interval
- **THEN** the mismatched effective mixer projection rejects that set without replacing prior PCM or audio
- **AND** a fresh capture and publication after the actual mixer clear can succeed under current nonaccepted authority
- **AND** neither control availability nor historical ticket metadata bypasses effective mixer compatibility

### Requirement: Admitted Same-Source Stems Share Current Effective Trajectory
The system SHALL retain admitted same-source stem PCM and refresh only its fixed
accepted projection on successful native adoption/clear. Pending, failed or
rejected timing SHALL preserve the previous projection. Rendering SHALL match
actual source and exact effective mixer timing. Full mix, stems and transitions
SHALL share one SourcePlayback trajectory; refresh SHALL preserve source
progression and physical endpoints without rebuilding PCM or adding a cursor.

#### Scenario: Same-source accepted timing is successfully replaced
- **GIVEN** admitted stems and active fractional SourcePlayback under current accepted timing
- **WHEN** the mixer successfully adopts a new accepted projection for the same actual source
- **THEN** retained stem PCM receives the new fixed effective revision, period and signed origin
- **AND** subsequent stem reads match current mixer timing
- **AND** all stems and full mix continue the same source trajectory without a PCM rebuild or playhead reset
- **AND** immutable same-source PCM owners remain intact while only fixed effective projection metadata changes
- **AND** every stem, full mix and both transition sides share fractional position, rate/ramp, interpolation taps and loop/seek policy
- **AND** refresh allocates no second cursor and rewrites no physical endpoints in realtime processing

#### Scenario: Same-source accepted authority is successfully cleared
- **GIVEN** admitted stems under an acknowledged accepted projection
- **WHEN** a successful native authority edit clears that projection for the same source
- **THEN** retained stems refresh their fixed effective projection to the native cleared state
- **AND** shared source-frame playback remains under current native policy without promoting Manual, Tap or Legacy numbers

#### Scenario: A retained stem projection does not match current mixer timing
- **GIVEN** retained stem PCM whose complete accepted revision, exact binary64 period or signed origin differs from effective mixer timing
- **WHEN** productive source_reader checks render compatibility
- **THEN** the mismatched set cannot supply stem audio under that projection
- **AND** matching source hash, shape or numerical loop endpoints cannot bypass the check

#### Scenario: A pending or rejected replacement accompanies stem rendering
- **GIVEN** admitted stems and one current acknowledged source trajectory
- **WHEN** another accepted record is pending, failed or rejected
- **THEN** admitted stems retain the previous effective accepted projection
- **AND** each interpolation tap for full mix and all stems uses the same current source position and wrap policy
