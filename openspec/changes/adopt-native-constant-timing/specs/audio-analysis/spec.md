## ADDED Requirements

G3b2f1 covers continuous productive voice/native/FIFO/filter-history ownership.
G3b2f2 adds the productive prepared native-history contract below. Its completion
requires actual worker-owned native/FIFO preparation and timed adoption evidence,
before G3b2g persistence; later B5 audible compensation is a separate gate.

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

### Requirement: Productive Native And FIFO History Owns Its Actual Source Feed
The system SHALL fill productive StretchProcessor feed from the actual borrowed
source through shared SourceReadPlan/SourcePlayback. Native history and pending
FIFOs SHALL bind source address/shape/loaded rate and full effective accepted
revision with bit-exact period/signed origin. It SHALL check expected fractional
next-source position before feed and invalidate foreign or discontinuous history.
A neutral warmed reserve SHALL NOT count as source history.

#### Scenario: The productive processor consumes accepted source audio
- **GIVEN** a voice pinned to an actual source with acknowledged accepted timing
- **WHEN** productive StretchProcessor rendering reads canonical fractional feed
- **THEN** actual native and pending FIFO history carry that source and complete effective accepted projection
- **AND** caller metadata, a raw revision or a warmed reserve cannot substitute for actual consumed feed
- **AND** a warmed reserve does not provide prepared source-specific priming

#### Scenario: A foreign source or discontinuous position reaches the processor
- **GIVEN** actual native or pending FIFO history from earlier productive feed
- **WHEN** the source owner or expected next fractional position differs before consumption
- **THEN** bounded adapter invalidation prevents foreign feed from entering the old history
- **AND** used native state is exchanged through the existing worker ownership lanes without callback reset, loading or destruction

#### Scenario: The worker reserve is unavailable
- **GIVEN** invalidated used native state with no admissible warmed reserve
- **WHEN** wet processing retries
- **THEN** existing bounded wet-silence behavior preserves native ownership and canonical source progression
- **AND** dry processing remains available without promoting the reserve to source history

### Requirement: Continuous Productive History Tracks Effective Timing Refresh
The system SHALL retain chronological native/FIFO history across continuous
same-source timing adoption/clear, rate changes, pause/resume and stem crossfades,
refreshing its full effective projection on productive feed. Pending, failed or
rejected timing SHALL NOT change that projection. Start/retrigger, stop, seek,
wet deactivation and source-position discontinuity SHALL invalidate adapter
history through existing bounded rules.

#### Scenario: Same-source accepted revision changes with equal timing numbers
- **GIVEN** consumed native/FIFO history and a continuous same-source trajectory
- **WHEN** a different full accepted revision with equal period/origin is successfully adopted
- **THEN** the next productive feed carries the new complete effective revision while retaining chronological history
- **AND** exact period and signed-origin bits remain part of the binding

#### Scenario: Accepted authority is successfully cleared
- **GIVEN** consumed same-source history under effective accepted timing
- **WHEN** native Manual, Tap or Legacy authority clears acceptance
- **THEN** subsequent continuous productive feed carries the cleared effective projection
- **AND** those authority values are not promoted to accepted evidence

#### Scenario: A replacement is pending or rejected
- **GIVEN** productive history under a current effective accepted record
- **WHEN** a new proposal is pending, fails admission or is rejected at callback adoption
- **THEN** effective source/timing and consumed history remain under the previous record
- **AND** Automatic without current acknowledgement cannot authorize new accepted preparation

#### Scenario: Rate, pause or stem selection changes preserve continuity
- **GIVEN** a continuous canonical fractional source trajectory
- **WHEN** rate smoothing advances, pause/resume occurs or source selection crossfades
- **THEN** native/FIFO history remains chronological under the same source ownership
- **AND** no second source cursor or physical endpoint rewrite is introduced
- **AND** this binding provides no prepared priming, audible delay compensation or seamless wet/bypass transition guarantee

### Requirement: Active Voice Timing Belongs To Its Pinned Source
The system SHALL bind voice timing/history and seek bounds to its actually pinned source.
A replacement bank SHALL NOT relabel old PCM with its accepted timing.
Retained old-voice ownership SHALL NOT be CURRENT pad-bank acceptance.
Retrigger SHALL adopt current bank PCM with off-realtime pin retirement.
Same-source effective timing refresh SHALL update only matching voices;
failed or rejected adoption SHALL preserve previous effective audio/history.

#### Scenario: A bank source is replaced while its old voice remains active
- **GIVEN** an active voice pinned to the previous bank source and its effective timing
- **WHEN** a replacement bank source and accepted revision become current
- **THEN** the old voice continues only under its pinned source and previous effective timing
- **AND** the replacement revision cannot relabel its old native/FIFO history
- **AND** this old effective ownership can continue until explicit retrigger or stop without becoming current pad-bank acceptance

#### Scenario: Explicit retrigger follows bank replacement
- **GIVEN** an active voice whose source differs from the current bank
- **WHEN** an admitted retrigger executes
- **THEN** the voice adopts the current bank PCM and effective timing
- **AND** old sample ownership retires through existing non-realtime paths
- **AND** old native/FIFO history cannot consume the replacement source as a continuation

#### Scenario: Same-source timing refresh occurs while a voice is paused
- **GIVEN** a paused voice pinned to the source of an admitted timing refresh
- **WHEN** native accepted timing is successfully adopted or cleared
- **THEN** its effective timing follows only that matching source
- **AND** consumed history remains frozen until productive feed resumes and refreshes its projection

#### Scenario: An explicit seek targets retained source audio after bank replacement
- **GIVEN** an active voice retaining a longer old source while the current bank contains a shorter replacement
- **WHEN** an explicit seek targets a frame in that voice's old source extent, or repeats its existing position
- **THEN** seek bounds and source progression use the actually pinned old source
- **AND** every successful explicit seek clears bounded native/FIFO adapter history and fixed per-pad filter ownership even if the source phase is unchanged
- **AND** native DSP reset/warming remains outside the callback on the preparation worker

### Requirement: New Voice Ownership Requires Available Effective Timing And Retirement
The system SHALL reject new voice/retrigger adoption if Automatic lacks current
acknowledgement or effective acceptance no longer matches acknowledgement.
It SHALL reserve off-realtime retirement capacity before replacement changes
old voice, loop or exclusive playback. Rejection SHALL preserve existing
audio/history. Manual/Tap/Legacy SHALL retain nonaccepted authority. The guarded
controller batch transaction below SHALL reuse these single-voice checks.

#### Scenario: Automatic timing has no current acknowledgement
- **GIVEN** Automatic authority without a current accepted projection
- **WHEN** a new voice or retrigger requests adoption of that bank source
- **THEN** adoption is rejected before replacing old source/history
- **AND** ordinary Legacy numbers cannot authorize an Automatic fallback launch

#### Scenario: Control authority retires before effective callback clearing
- **GIVEN** an old accepted projection still effective in the mixer after control revocation
- **WHEN** another new voice or retrigger requests adoption during that interval
- **THEN** mismatch with current acknowledgement rejects new adoption
- **AND** the already effective voice continues until the admitted callback clear executes
- **AND** fresh Manual/Tap/Legacy adoption can proceed after the effective clear under its own authority

#### Scenario: Replacement has no retirement capacity
- **GIVEN** an old active voice pin and an otherwise valid replacement bank
- **WHEN** required off-realtime retirement capacity is unavailable
- **THEN** replacement adoption fails before old voice, loop or exclusive playback changes
- **AND** old effective source/timing/history remains available

### Requirement: New Voice Adoption Requires Current Native Source Ownership
The system SHALL check current native source ownership before new voice/retrigger
adoption. An unavailable loading source or a control source that differs from the
effective callback bank SHALL reject new adoption before old voice, loop or
exclusive playback changes. Ongoing effective playback SHALL retain its pinned
source/timing/history during that interval. The check SHALL be bounded and SHALL
NOT infer ownership from caller metadata or a historical ticket.

#### Scenario: Native source loading has made control ownership unavailable
- **GIVEN** an old effective voice while native loading has made current control source ownership unavailable
- **WHEN** another new start or retrigger requests adoption
- **THEN** unavailable actual native source ownership rejects that adoption
- **AND** ongoing old effective source/timing/history remains available
- **AND** Legacy numerical fallback cannot bypass the source ownership check

#### Scenario: Replacement control PCM is published before callback bank adoption
- **GIVEN** replacement PCM current under native control ownership while the callback bank still contains the previous source
- **WHEN** another new start or retrigger requests adoption before the bank changes
- **THEN** source ownership mismatch rejects that adoption before loop or exclusive playback changes
- **AND** old ongoing history cannot be relabeled with the replacement's timing
- **AND** fresh adoption can proceed only after actual current source ownership and callback bank agree

### Requirement: Productive Per-Pad Filter History Uses The Same Source Trajectory
The system SHALL bind actual per-pad filter history to the rendered voice's
source/shape/loaded rate, full effective accepted projection and fractional
next-source position. Continuous same-source timing/rate refresh SHALL retain
filter state and refresh the projection. Foreign source or discontinuity SHALL
clear fixed Rust filter storage before replacement output. Its ledger SHALL
count filtered output, including wet fallback silence, without asserting
audible source content.

#### Scenario: Continuous accepted refresh reaches the productive filter chain
- **GIVEN** a per-pad filter has processed the voice's previous canonical chunk
- **WHEN** a same-source effective revision or rate target changes without source discontinuity
- **THEN** actual filter state is retained with the complete new effective projection
- **AND** source trajectory remains the voice's shared canonical cursor

#### Scenario: A replacement voice reaches old per-pad filter state
- **GIVEN** nonzero filter history from the previous voice source
- **WHEN** a foreign source or discontinuous fractional position reaches productive filtering
- **THEN** fixed filter storage is cleared before replacement output is processed
- **AND** old source filter state cannot color the replacement as continued history

#### Scenario: Wet output is silent while a native reserve is unavailable
- **GIVEN** the existing wet fallback emits bounded silence while canonical source progression continues
- **WHEN** productive per-pad DSP processes those output frames
- **THEN** filter history records the actual processed output and current source trajectory
- **AND** its binding does not claim those silent frames contain audible source feed

### Requirement: Productive History Preserves Existing Realtime Native Ownership
The system SHALL keep productive history checks, source reads, projection copies
and fixed adapter/filter storage changes bounded. Native DSP construction/reset/
loading/warming and large-owner retirement SHALL remain outside realtime
processing. History binding SHALL reuse existing worker exchanges and
off-realtime sample retirement without adding callback locks, evidence allocation,
PCM hashing, I/O, Python/UI access or logging.

#### Scenario: Productive history becomes discontinuous or a voice source changes
- **GIVEN** productive native/adapter/filter history and an owning source pin
- **WHEN** bounded history invalidation or source replacement runs on the callback
- **THEN** only fixed adapter/filter storage and ownership checks change there
- **AND** native reset/warming and large-owner destruction use the existing worker/retirement paths
- **AND** callback processing adds no locks, evidence allocation, PCM hashing, I/O, Python/UI access or logging
- **AND** stop/unload clears filter history ownership before the voice pin retires without retaining an additional PCM or native handle

### Requirement: Productive Source Preparation Retains Actual Native And FIFO History
The system SHALL prepare productive Key Lock replacement history on its native
worker from pinned actual source/stem PCM, a copied canonical SourcePlayback and
the shared SourceReadPlan. The prepared owner SHALL retain the actual processed
native handle and pending adapter FIFOs. Neutral warming, opaque tags or test-only
state SHALL NOT satisfy productive source-specific preparation.

#### Scenario: The worker prepares actual productive source history
- **GIVEN** an eligible effective voice and a captured current source/timing permit
- **WHEN** the native worker prepares and catches up a replacement through the canonical source reader and adapter
- **THEN** its retained handle has consumed that actual fractional source/stem feed
- **AND** its native state and input/output FIFOs form one coherent continuation owned until adoption or off-thread retirement
- **AND** genuine shifted output and handle/FIFO ownership are independently verifiable

### Requirement: Prepared Native History Requires Current Complete Permits
The system SHALL bind prepared native history to current source generation/load
request, loaded rate, preparation request/epoch, declared authority and the full
CURRENT acknowledged accepted revision with exact binary64 period and signed
origin. It SHALL recheck this permit at actual adoption. Manual/Tap/Legacy SHALL
remain nonaccepted; unavailable Automatic SHALL fail admission.

#### Scenario: A current accepted permit is captured
- **GIVEN** actual source ownership under current acknowledged Automatic timing
- **WHEN** productive native preparation captures its permit
- **THEN** it binds source generation/load request/rate, shared preparation request/epoch, authority/runtime revisions and complete exact accepted projection
- **AND** equal timing numbers or signed-zero changes cannot substitute for that complete projection

#### Scenario: Automatic is unavailable or only historical acceptance is supplied
- **GIVEN** Automatic authority without consistent current native acknowledgement
- **WHEN** productive native preparation is requested
- **THEN** no accepted preparation is admitted from historical ticket metadata, raw revisions or equal numerical timing
- **AND** previous effective audio/native history remains available

#### Scenario: Nonaccepted authority requests preparation
- **GIVEN** a current Manual, Tap or Legacy source binding
- **WHEN** eligible source-specific history is prepared
- **THEN** preparation remains under that nonaccepted authority
- **AND** its numerical period or origin is not promoted to an accepted revision

### Requirement: Prepared Native Adoption Is Timed And Transactional
The system SHALL adopt a prepared native/FIFO owner only at its exact absolute
output-frame deadline, splitting productive rendering at that boundary. It SHALL
recheck current source/timing and complete runtime trajectory and reserve off-thread
recycling before exchange. Worker catch-up SHALL finish before adoption; late,
unready, stale, failed or rejected work SHALL preserve previous effective audio/
native history without partial replacement.

#### Scenario: Ready prepared history reaches its exact adoption frame
- **GIVEN** a ready source-specific native/FIFO owner matching the current effective voice and captured permit
- **WHEN** a callback spans the absolute adoption deadline
- **THEN** the callback renders the old effective owner up to the exact frame and rechecks all captured runtime/current-authority constraints
- **AND** those checks include physical loop, fractional trajectory/seek mode, source/stem selection and rate
- **AND** it exchanges the complete native/FIFO continuation only after recycling capacity is reserved
- **AND** subsequent output uses the adopted native owner and the same canonical logical source trajectory

#### Scenario: A control or source change retires preparation
- **GIVEN** source-specific preparation pending under an earlier source, loop, seek, stem, rate or accepted binding
- **WHEN** that runtime state or current source/request/epoch/authority changes before adoption
- **THEN** the mismatching prepared owner cannot enter productive rendering
- **AND** the previously effective native/FIFO owner and audio remain available
- **AND** even equal period/origin numbers cannot hide a changed full accepted revision

#### Scenario: Readiness or recycling fails at the deadline
- **GIVEN** an existing effective owner and a prepared adoption deadline
- **WHEN** preparation fails, completion is late, the result is unready or the bounded recycle lane is full
- **THEN** no partial replacement changes effective audio/native history
- **AND** failed or rejected prepared source/native/FIFO owners retire outside the callback

### Requirement: Prepared Native Work Remains Outside Realtime Rendering
The system SHALL perform source-specific priming, catch-up, native construction,
reset, cold pitch setup, heavy source processing and prepared-owner destruction
on non-realtime paths. The callback SHALL use only bounded queue/permit/runtime
checks, exact boundary splitting and ownership exchange through existing native
worker and retirement mechanisms, without heavy preparation, blocking locks,
PCM hashing, I/O, Python/UI access, logging or large-owner destruction.

#### Scenario: Prepared native work is superseded or rejected
- **GIVEN** a prepared native handle/FIFO owner and pinned actual source/stem PCM
- **WHEN** preparation is cancelled, fails or is rejected at timed adoption
- **THEN** native and large PCM owners remain retained for off-thread recycling/destruction
- **AND** callback rejection performs bounded work without native reset, loading or heavy catch-up

### Requirement: Cancelled Prepared Native Owners Retire Without Source Rendering
The system SHALL check a shared local voice epoch before and after worker catch-up
and the exact outstanding request ID before adoption. Stop/reset/wet deactivation
SHALL cancel pending work; discarded jobs SHALL settle pending state through fixed
completion feedback. Inactive/paused callbacks SHALL poll bounded owner retirement
without source rendering; source/native/FIFO destruction and teardown tails SHALL
remain off realtime.

#### Scenario: Cancellation races with worker preparation or publication
- **GIVEN** a prepared job in flight or ready while its voice stops, resets or leaves wet processing
- **WHEN** local cancellation is published before or after the worker's final freshness check
- **THEN** stale in-flight work is discarded on the worker, or bounded callback retirement returns the late published owner intact
- **AND** completed-request feedback settles pending work without allowing an old request ID to adopt

#### Scenario: A cancelled voice never renders source audio again
- **GIVEN** a cancelled inactive or paused voice with a late ready result or retained source/stem pins
- **WHEN** subsequent callbacks poll its bounded retirement lanes, or stream teardown runs after rendering stops
- **THEN** native/FIFO/source/stem owners retire off realtime without requiring another productive source feed
- **AND** the dirty-state fence prevents invalidated native history from consuming foreign feed

### Requirement: Controller Global Batches Bind Every Current Native Pad
The system SHALL bind each controller/MIDI GLOBAL START/STOP pad to actual native
source/authority and full CURRENT acknowledged accepted revision, exact period and
signed origin. Admission SHALL reject the whole batch for unavailable, stale,
foreign or inconsistent bindings. Manual/Tap/Legacy SHALL remain nonaccepted;
historical metadata and equal numerical timing SHALL NOT establish ownership.

#### Scenario: Several current accepted pads restart together
- **GIVEN** active or remembered pads with actual native current source ownership and acknowledged accepted timing
- **WHEN** GLOBAL START captures their batch
- **THEN** every affected pad carries its actual source generation/content/rate/extent and declared authority
- **AND** each Automatic pad carries its full current accepted revision, exact period and signed origin
- **AND** one operation uses the matching frozen Python timing snapshot for each pad's loop calculation

#### Scenario: One pad is unavailable or changes during admission
- **GIVEN** an otherwise valid batch with one Automatic pad lacking consistent current acknowledgement or one retired source/authority binding
- **WHEN** native batch admission validates all affected pads
- **THEN** the entire batch is rejected without publishing any pad loop or playback effect
- **AND** previous effective audio, loop, transport and controller restore state remain available

#### Scenario: A replacement is pending or rejected
- **GIVEN** a current accepted record and a replacement proposal that is pending or rejected
- **WHEN** a controller batch captures current timing
- **THEN** the previous acknowledged accepted revision remains authoritative
- **AND** the proposal or historical ticket does not become batch authority

#### Scenario: Manual Tap and Legacy participate in a mixed batch
- **GIVEN** actual current native sources under Manual, Tap, Legacy and acknowledged Automatic authority
- **WHEN** the controller captures one mixed batch
- **THEN** the first three pads remain bound to their own nonaccepted authority
- **AND** matching numerical timing cannot promote them to accepted records

### Requirement: Global Batch Execution Is Transactional And Scheduled Once
The system SHALL admit GLOBAL START/STOP as one bounded transaction, recheck all
source/authority/accepted bindings at execution and schedule START at one shared
output boundary with its captured timestamp. Execution SHALL reserve whole-batch
voice, retirement and playback-feedback capacity before effects. Validation or
command/feedback/scheduler/capacity failure SHALL preserve previous audio, loops,
transport and ownership without partial effects.

#### Scenario: Quantized restart reaches its common boundary
- **GIVEN** a valid admitted batch and one captured input timestamp
- **WHEN** the native scheduler reaches the selected output frame
- **THEN** it rechecks the complete batch before any loop or voice mutation
- **AND** all affected starts use that same output frame and their matching loop intent
- **AND** source rates, fractional progression, DSP invalidation and pin retirement reuse existing single-pad behavior

#### Scenario: An equal-valued revision changes after enqueue
- **GIVEN** an admitted batch awaiting immediate or quantized execution
- **WHEN** one source, declared authority, full accepted revision or signed-origin bits changes before execution despite equal BPM or loop endpoints
- **THEN** the whole stale batch is rejected before any affected pad changes
- **AND** unrelated and previously effective pinned voices remain available

#### Scenario: Capacity fails before the batch can commit
- **GIVEN** current effective voices, loops and transport state
- **WHEN** command or playback-feedback capacity is insufficient, the scheduler is full, available voices are insufficient or off-realtime retirement capacity cannot cover the entire batch
- **THEN** no subset of loops, starts, stops or transport bootstrap is applied
- **AND** source/native/FIFO/filter owners remain coherent and large owners retire only outside realtime processing

#### Scenario: Playback feedback cannot cover every affected pad
- **GIVEN** a current valid START or STOP batch and a bounded native-to-control feedback ring
- **WHEN** its available slots are fewer than all required SampleStarted or SampleStopped messages
- **THEN** execution rejects the whole batch before changing audio, loops or pinned ownership
- **AND** it publishes no partial playback feedback or accepted result
- **AND** previous controller restore intent remains available

#### Scenario: Current bank and an active pinned voice have different ownership
- **GIVEN** a voice retaining an older source and effective timing after current bank replacement
- **WHEN** a guarded global batch is checked or committed
- **THEN** the replacement bank's current accepted record cannot relabel the old voice or its history
- **AND** stale or inconsistent ownership rejects the batch without partially changing other voices
- **AND** an admitted restart adopts the actual current bank source through the existing preparation and retirement path

### Requirement: Controller Batch Feedback Preserves Restore State Until Acceptance
The system SHALL distinguish enqueue/scheduling from native acceptance.
Active/paused state SHALL follow playback messages; SampleStarted SHALL clear
paused state. Only accepted batches SHALL change restore intent; pending/rejected
work SHALL preserve it. STOP SHALL remember the playing subset. Subsequent global
target capture SHALL first drain existing playback feedback. Unload SHALL prune
observed/pending restore and publish reliable ordered STOP feedback.

#### Scenario: Stop is admitted but not yet executed
- **GIVEN** an active playing set and paused pads with existing GLOBAL restore bookkeeping
- **WHEN** GLOBAL STOP is enqueued or scheduled but native execution remains pending
- **THEN** controller bookkeeping retains the previous active, paused and restore state
- **AND** only actual accepted execution remembers the playing subset and applies the stop result

#### Scenario: Remembered restart is rejected after enqueue
- **GIVEN** a remembered restore set and a pending GLOBAL START batch
- **WHEN** source or authority changes before native execution rejects that batch
- **THEN** controller feedback retains the remembered restore set and prior active/paused state
- **AND** the rejection cannot consume the next legitimate restore attempt

#### Scenario: A pad unloads before its accepted STOP is observed
- **GIVEN** a GLOBAL STOP ticket that becomes accepted before Python observes it
- **WHEN** the loader unloads or replaces one captured pad before ticket polling
- **THEN** the loader's control-intent callback removes that pad from observed and pending restore sets
- **AND** later accepted feedback can remember only the remaining captured pads
- **AND** restore engagement is false when no remembered pads remain

#### Scenario: Restart acknowledgement precedes native playback telemetry
- **GIVEN** a remembered pad whose session projection is still paused
- **WHEN** accepted START feedback is observed before its SampleStarted message
- **THEN** ticket polling does not optimistically change active or paused state
- **AND** the actual SampleStarted message adds the active pad and clears its paused projection

#### Scenario: Mapped MIDI global actions execute productively
- **GIVEN** a mapped MIDI GLOBAL START or STOP action and its captured Rust input timestamp
- **WHEN** input handling executes the controller action
- **THEN** it uses the same source-bound all-or-none batch transaction as the UI controller
- **AND** no direct unguarded MIDI stop-all or Python per-pad loop/play sequence bypasses that transaction
- **AND** pending or failed execution cannot optimistically corrupt session state

#### Scenario: A following STOP arrives before normal START feedback polling
- **GIVEN** an accepted remembered START with native SampleStarted messages still queued
- **WHEN** a following MIDI, keyboard or UI global STOP captures its targets before normal frame audio polling
- **THEN** it first drains the existing playback feedback after observing batch completion
- **AND** it captures the complete newly active set for the same guarded STOP transaction

#### Scenario: Unload follows an older queued START message
- **GIVEN** an accepted START with its feedback queued and a subsequently admitted pad unload
- **WHEN** the native callback executes the unload
- **THEN** it reserves one feedback slot before any unload effect and emits ordered SampleStopped feedback
- **AND** full feedback defers the entire unload with prior pins and audio preserved
- **AND** draining the older START and subsequent unload STOP leaves that pad inactive

### Requirement: Global Batch Realtime Work Is Fixed And Bounded
The system SHALL allocate batch/scheduler storage outside realtime and bound
callback work by configured capacity, reusing existing trajectory, preparation
and retirement. Batch callback work SHALL add no PCM scans, evidence allocation,
locks, GIL/UI/I/O/logging, native DSP construction/reset or large-owner destruction.
Feedback SHALL NOT drive audio timing, accepted publication or derived loop/master
refresh, which remains a separate integration boundary.

#### Scenario: A batch executes or retires on the callback
- **GIVEN** a captured fixed-capacity controller batch
- **WHEN** callback admission, scheduling, execution, rejection or feedback runs
- **THEN** work remains bounded by configured pad/voice/batch capacity
- **AND** evidence, native preparation and large-owner retirement stay off realtime
- **AND** batch feedback does not become a second timing owner or an automatic acceptance orchestrator
