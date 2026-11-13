# background-tasks Specification

## Purpose
To define non-realtime per-pad background operations, progress reporting, and conflict prevention for tasks such as analysis and offline stem generation.
## Requirements
### Requirement: Background Tasks Support Per-Pad Operations
The system SHALL support running non-real-time operations for a pad as background tasks with progress reporting.

Background tasks MAY be part of the sample loading pipeline (automatic) or MAY be triggered manually for an already-loaded pad.

#### Scenario: A pad runs an analysis-only background task
- **GIVEN** a pad has a loaded audio sample
- **WHEN** the user triggers "Analyze audio"
- **THEN** the system schedules an analysis-only background task for that pad
- **AND** the UI receives progress updates for that task

#### Scenario: A pad runs a background task that is not part of loading
- **GIVEN** a pad has a loaded audio sample
- **WHEN** the user triggers a future operation such as "Generate stems"
- **THEN** the system schedules a background task for that pad
- **AND** that task runs without re-running the sample loading pipeline

### Requirement: Background Task Concurrency Rules
The system SHALL prevent conflicting background operations for the same pad.

#### Scenario: Manual tasks are blocked while loading is in progress
- **GIVEN** a pad is currently loading
- **WHEN** the user tries to start a manual background task for that pad
- **THEN** the task is rejected or deferred


<!-- Added from add-offline-stem-cache -->
### Requirement: Stem Generation Runs As A Per-Pad Background Task
The system SHALL run stem generation as a non-real-time per-pad background task with
progress and error reporting.

Stem generation SHALL NOT run as part of the audio callback. It SHALL NOT block UI
rendering while the task is in progress.

#### Scenario: Stem task reports progress
- **GIVEN** a stopped, playing or paused pad has loaded source audio and no conflicting task
- **WHEN** stem generation starts
- **THEN** the system reports task progress for that pad outside the audio callback
- **AND** the UI remains responsive while generation runs


<!-- Added from add-offline-stem-cache -->
### Requirement: Stem Tasks Respect Per-Pad Concurrency
The system SHALL prevent conflicting per-pad tasks from running concurrently with stem
generation.

A stem generation request SHALL be rejected or deferred while the same pad is loading,
unloading, analyzing, or already generating stems. Active playback or a retained
paused voice on a current subscriber fences heavy inference under the shared
resource admission below.
Compatible equal-material jobs MAY share physical work while preserving each
subscriber's independent current request and source fences.

#### Scenario: Loading pad blocks stem generation
- **GIVEN** a pad is currently loading source audio
- **WHEN** the performer requests stem generation for that pad
- **THEN** the system rejects or defers the stem task
- **AND** the load task continues without being replaced by stem generation

#### Scenario: Existing stem task blocks another stem task
- **GIVEN** stem generation is already running for a pad
- **WHEN** the performer requests stem generation again for that pad
- **THEN** the system rejects or defers the duplicate task
- **AND** no second conflicting stem generation task is started for that pad


<!-- Added from add-offline-stem-cache -->
### Requirement: Stem Task Completion Is Revalidated Before Publication
The system SHALL revalidate each subscriber's current source, content, request and
material/model intent before accepting a completed stem task result. A running backend
SHALL retain its source/job leases through actual return even if all public interests end.

Disk verification and saved selection SHALL NOT establish effective audio adoption.
Playing or paused pads SHALL require their own supported Native adoption transaction,
fresh timing/window/voice/history permits and own ACK. Unloaded, replaced or cancelled
subscribers SHALL NOT receive stale publication; existing readers remain protected.

#### Scenario: Completed task is stale
- **GIVEN** stem generation started for pad source version A
- **AND** the pad is replaced with source version B before generation completes
- **WHEN** the task finishes
- **THEN** the result for source version A is marked stale
- **AND** it is not published to the audio thread for source version B


<!-- Added from add-stem-performance-controls -->
### Requirement: Performer Stem Generation Uses Background Tasks
The system SHALL route performer stem-generation requests through the non-real-time per-pad
background-task path.

The request SHALL respect existing loading, analysis, stem-generation and unloading
gates. Playback headroom SHALL be checked separately before and during inference.
Progress, success, and failure SHALL be reported
through controller/session state so the UI can render status without blocking.

#### Scenario: Generate stems action starts a background task
- **GIVEN** a loaded pad has no conflicting per-pad task
- **WHEN** the performer requests stem generation from the UI
- **THEN** the system schedules a per-pad stem-generation background task
- **AND** UI rendering remains responsive while the task runs

#### Scenario: Conflicting task blocks UI stem generation
- **GIVEN** a pad is currently loading, analyzing, already generating stems, or unloading
- **WHEN** the performer requests stem generation from the UI
- **THEN** the request is rejected or deferred through controller state
- **AND** no stem generation work is run by the audio callback


<!-- Added from harden-gen3-runtime-control-paths -->
### Requirement: Per-pad background completions use current request identity
The system SHALL attach an accepted pad request identity to load, analysis, and per-pad background task events, and SHALL apply completion results only when that identity still matches current pad intent.

The identity SHALL be invalidated when the pad is unloaded, when a replacement load is accepted for the same pad, or when controller state otherwise clears the source that the background task was created for.

#### Scenario: Replaced load completion is ignored
- **GIVEN** load request A is accepted for pad 1
- **AND** replacement load request B is accepted for pad 1 before request A completes
- **WHEN** request A later emits progress, success, error, analysis, or publication results
- **THEN** the system ignores request A for pad-state mutation
- **AND** request A does not replace the Rust sample cache or publish `LoadSample` into the audio command path
- **AND** request B remains the current pad intent

#### Scenario: Unloaded load completion is ignored
- **GIVEN** a load request is in progress for pad 2
- **WHEN** the performer unloads pad 2 before the request completes
- **AND** the stale load later emits a success or error
- **THEN** pad 2 remains unloaded
- **AND** stale progress, error, cached path, duration, and analysis data are not applied to project or session state

#### Scenario: Manual analysis completion is source-guarded
- **GIVEN** manual analysis is running for pad 3 and source X
- **WHEN** pad 3 is unloaded or replaced with source Y before analysis completes
- **AND** the analysis task for source X later succeeds
- **THEN** the system ignores the source X analysis result
- **AND** source Y analysis state is not overwritten

### Requirement: Heavy Background Work Shares Resource Admission
Existing offline owners SHALL coordinate heavy stem generation and analysis child
execution without a second scheduler, job queue or audio clock. Already-admitted
analysis SHALL wait cancellably for heavy capacity; resource contention SHALL NOT
become numerical failure or select a lower-precision BPM path. Effective child
numerical pools SHALL be bounded before model work, with child environment applied
before imports and no mutation of App-global numerical settings.

Windows stem command ownership SHALL assign the child to its kill-on-close Job
while suspended and retain the Job through actual tree and pipe-reader retirement.
The stem guard SHALL use actual Windows low-memory status and exhausted observed
headroom for admission and active stop. Stem command Jobs SHALL run without a RAM
or commit ceiling. Alignment SHALL account for the concrete block and outstanding
Native promises against measured capacity.

#### Scenario: Generation competes with playback or analysis
- **GIVEN** an original source-owned generation remains current
- **WHEN** a current requested subscriber is playing or retains a paused voice before inference admission
- **THEN** inference waits visibly without stopping any PAD or changing its original request
- **AND** saved-set lookup and component preparation retain independent admission
- **WHEN** a current requested subscriber's playback becomes active during an admitted generation
- **THEN** that attempt reports a distinct playback-headroom failure and retains custody until actual cleanup
- **AND** a waiting precise analysis retains its original quality and cancellation authority

#### Scenario: Windows signals low memory during a stem command
- **GIVEN** caller/system commit and physical headroom are measured separately
- **WHEN** the existing stem guard observes low memory or exhausted headroom
- **THEN** the owned command requests cancellation and retires its Job, tree and both readers
- **AND** the original attempt reports resource pressure after retirement
- **AND** unsupported platform monitoring and unmeasured device deadlines remain explicit

#### Scenario: A queued stem becomes ineligible for memory admission
- **GIVEN** a current inference, requested pair preparation or held reuse delivery owns an inactive common FIFO position
- **WHEN** the existing stem resource guard reports pressure or unavailable monitoring
- **THEN** the controller SHALL withdraw only that inactive position so an otherwise admitted background owner can progress
- **AND** the original request, interests, source custody and held delivery remain pending without failure or cancellation
- **AND** recovery rejoins the same owner behind existing FIFO waiters when monitoring reports clear
- **AND** active tickets and independent borrows remain owned until ordinary retirement, while failed deferred terminal deliveries retain their separate custody

### Requirement: Heavy Stem And Analysis Jobs Execute Sequentially
PAD stem generation, Generate all Stems and complete BPM/Key analysis SHALL share
one sequential heavy-work admission while retaining their existing bounded queues.
The next independent heavy job SHALL NOT start PCM/model preparation until the
previous job's transient owners, actual child trees and readers have retired.
Ordinary playback preparation SHALL remain eligible while heavy work waits.
Same-job branches MAY overlap only under the same retained admission and SHALL
both end before that admission is released. Cancellation and UI completion alone
SHALL NOT certify resource release. Persistent playback sources and published
results SHALL retain their valid ownership; no OS RAM trimming is required.

#### Scenario: PAD and batch stems compete with complete analysis
- **GIVEN** one Stem or BPM/Key job still owns transient model/PCM storage or a child/reader
- **WHEN** more PAD requests, Generate all Stems or Analyze Audio are requested
- **THEN** their existing bounded queues retain requests without starting another independent heavy job
- **AND** a cancelled queued request releases only its own admission and cannot strand later work
- **AND** the successor starts only after actual prior owner retirement, including error and shutdown paths
- **AND** ordinary Cold playback preparation may progress while the heavy job remains occupied

#### Scenario: Available host capacity changes
- **GIVEN** an eligible heavy job and other live owners with outstanding reservations
- **WHEN** the system evaluates host memory admission
- **THEN** it uses observed physical memory, commit and applicable OS pressure signals with those actual reservations
- **AND** Windows model workers run in owned Jobs without a fixed host-memory ceiling
- **AND** original source geometry, numerical quality and last-reader guards remain authoritative

#### Scenario: Beat output setup fails after its child exists
- **GIVEN** an admitted Beat worker has created its child process
- **WHEN** stdout validation or output-reader construction fails, or an unexpected setup or supervision exception propagates
- **THEN** the existing process owner still terminates and reaps the child and settles its readers
- **AND** temporary storage, Beat slots and the same heavy-work permission remain owned until actual retirement, including delayed cleanup
- **AND** another heavy job cannot start early, while a fresh Beat request can run after the failed attempt's resources retire

#### Scenario: Native heavy admission fails before Beat work begins
- **GIVEN** a Beat invocation has not begun payload work or created a child
- **WHEN** Native heavy-ticket construction or acquisition reports an admission failure
- **THEN** the invocation reports terminal supervision failure and settles its own retirement event
- **AND** both CPU and CUDA protocol bridges can return without waiting for nonexistent readers
- **AND** a diagnostic analysis job retires its captured Native PCM and reports failure without starting either branch or fabricating cancellation
- **AND** caller callback and admitted-payload exceptions keep their original identity, while actual live tree and reader ownership remains protected

#### Scenario: Diagnostic cancellation bridge construction fails
- **GIVEN** diagnostic analysis owns captured Native source admission and has not started either analysis branch
- **WHEN** cancellation-thread construction raises an error covered by existing supervision failure handling
- **THEN** the job follows its ordinary Native PCM retirement and terminal failure settlement without exporting PCM, starting inference or fabricating cancellation
- **AND** an existing admission failure remains authoritative without constructing the optional bridge
- **AND** prior cancellation remains cancellation, and a real Native retirement refusal remains nonterminal with any admitted heavy borrow retained
- **AND** another heavy owner can acquire only after the original job's actual required retirement completes

#### Scenario: Other PADs play while stems are generated for a stopped PAD
- **GIVEN** the requested stem PAD is stopped and resource admission permits background work
- **WHEN** other PADs play or the performer operates their START/STOP controls
- **THEN** that unrelated playback does not defer or abort the stem calculation
- **AND** current requested subscribers, original source/cancellation, actual memory pressure and publication guards remain authoritative
- **AND** no PAD is stopped automatically to make background work eligible

#### Scenario: A shared job retains an obsolete playing subscriber
- **GIVEN** a shared physical stem job retains a stopped current subscriber and another subscriber whose source or content has changed
- **WHEN** the obsolete subscriber's PAD plays while its old readers are still retained
- **THEN** that unrelated current playback does not defer or abort the surviving subscriber's calculation
- **AND** currentness is checked against the existing complete source, content, placement and intent authority without releasing the obsolete reader's custody
- **AND** an active interested subscriber in an unsettled Pair mirror continues to fence work until that mirror settles
- **AND** heavy admission, memory pressure, cancellation and final publication checks retain their existing authority

### Requirement: General Windows Analysis Workers Use Observed Host Memory
The general Python analysis worker SHALL use a Windows Job without a model
memory limit. Entry and pre-resume checks SHALL use live caller and system
commit, physical headroom, and the OS low-memory signal. Active supervision
SHALL stop its owned work on observed pressure or sustained monitoring failure.
Scratch admission SHALL check space for the actual request file and report
disk exhaustion and allocation failures separately from pressure, monitoring
failure and cancellation. Existing Linux ownership behavior, bounded protocol
output, source lifetime, cancellation, and settlement SHALL remain in force.

#### Scenario: Windows reports memory pressure during analysis
- **GIVEN** a general Windows analysis worker owns a running process tree and both output readers
- **WHEN** live observation reports OS memory pressure
- **THEN** the supervisor stops that owned tree and reports resource pressure
- **AND** actual tree and reader retirement precede releasing the request files and admission

### Requirement: Shared Offline Workers Require Process Tree Ownership Before Launch
The shared Beat/stem worker launcher SHALL refuse execution on platforms without
an owning process-tree adapter before allocating a child or tree owner. Launcher
exit and closed output pipes SHALL NOT substitute for descendant retirement.
Refusal SHALL use existing startup-failure cleanup to settle unused admission,
temporary storage and ticket borrows. Windows and Linux ownership routes SHALL
retain their existing behavior. Platform support requires an owning adapter and validation.

#### Scenario: The host has no owning offline process adapter
- **GIVEN** an offline command or Beat request on an unsupported host such as macOS
- **WHEN** its shared launcher attempts to start the worker
- **THEN** it refuses before creating a child, native memory budget or tree owner
- **AND** the unused request's temporary storage, ticket borrows and admission settle so a later request is not stranded

### Requirement: Linux Offline Workers Own Descendants Through Reader Retirement
Linux offline supervision SHALL bind any observer to the gated original payload
before execution. A private subreaper SHALL retain descendants independently of
payload exit, process groups and sessions, reap its complete private subtree and
acknowledge ECHILD before successful broker exit can certify tree retirement.
Final acknowledgement SHALL drain racing teardown before closing the proof channel.
Payload PID and returncode SHALL remain distinct from broker PID and exit.
Existing admission, temporary storage and source custody SHALL remain held until
both actual tree retirement and independent output-reader settlement. Lost or
invalid ownership proof SHALL remain nonterminal and SHALL NOT admit a successor.
The application SHALL NOT become a global subreaper or reap unrelated children.

#### Scenario: A successful payload leaves an escaped or silent descendant
- **GIVEN** a Linux worker has inherited-output or silent descendants, including a changed session or double fork
- **WHEN** its original payload exits, fails, times out or is cancelled
- **THEN** its private broker terminates and reaps the complete owned subtree while unrelated processes remain alive
- **AND** payload status, genuine tree end, broker exit and both output-reader ends remain independent requirements

#### Scenario: Observer admission fails before Linux payload activation
- **GIVEN** the broker retains a gated payload and its stable original identity
- **WHEN** observer admission fails or cancellation is observed before activation
- **THEN** no payload code executes and private teardown still precedes resource release
- **AND** an externally killed gated payload remains unreaped until its identity can no longer be bound to a new observer

#### Scenario: Linux ownership proof is lost
- **GIVEN** an admitted invocation still owns its original source and resources
- **WHEN** the private broker disappears or its protocol cannot prove complete tree retirement
- **THEN** root exit or output EOF cannot certify retirement or release the original admission
- **AND** closing private control requests cleanup from a still-owning broker without inventing a tree-end receipt
- **AND** this bounded Linux ownership contract does not establish whole Linux application admission or macOS support

### Requirement: Slow Stem Work Does Not Own Unrelated Live Controls
Stem disk preparation and numeric conversion SHALL run outside realtime and
SHALL NOT retain shared control-publication locks across slow file reads or
conversion. Native numeric work SHALL release Python execution while running.
Short admission/publication sections SHALL revalidate current source, content,
placement, intent, memory reservations and operation-specific acknowledgement.

#### Scenario: A bounded numeric stem block is converted
- **GIVEN** four retained immutable PCM16 source spans and complete prospective raw/payload/copy/scratch promises
- **WHEN** the Python-independent native core converts a bounded block outside interpreter execution
- **THEN** global binary64 interpolation, compensated mono averaging, ties-even PCM16 endpoints and ordered instrumental clipping match the independent byte oracle
- **AND** unrelated mutable public controls, their own trigger/audio and STOP can progress
- **AND** cancellation during native work is observed before any subsequent output write, while worker/source custody lasts through actual return
- **AND** exclusive RIFF/RF64 writers, complete output verification and private generation commit remain authoritative
- **AND** the actual compiled numeric/bridge implementation participates in the processing identity; unsupported binding evidence remains UNKNOWN

#### Scenario: Components load while another PAD is played
- **GIVEN** one PAD is preparing components under a controlled slow reader
- **WHEN** another loaded PAD receives trigger or STOP
- **THEN** those controls can reach their ordinary admission path independently of the slow read
- **AND** same-PAD ordering, cancellation, immutable sharing and last-reader retirement remain correct

#### Scenario: A committed component request waits in native admission
- **GIVEN** a public committed residency request has captured its source-bound admission state
- **WHEN** its native admission phase waits outside Python
- **THEN** it retains no Python Engine borrow or interpreter execution
- **AND** source, placement, launch, selection and reservations are revalidated before mutation
- **AND** an older captured request cannot supersede a newer admitted intent
- **AND** a failed newer admission does not revoke an earlier viable request or current proposal

#### Scenario: A cancelled committed request remains behind occupied workers
- **GIVEN** both cold workers are occupied and an unclaimed committed residency request holds its actual callback capture in the queue
- **WHEN** its own receipt is cancelled and the ordinary loader control pump runs
- **THEN** the original queued callable and its captured readers retire off realtime before its cold admission becomes available
- **AND** replacement admission does not wait for either occupied worker to finish
- **AND** independent queued work, current selection authority and original acknowledgements remain intact
- **AND** an uncancelled lane close retains the existing job guard settlement, while claimed readers keep their original lifetime

#### Scenario: Intent changes during a committed component read
- **GIVEN** a worker holds immutable source and file pins with a scoped component reservation
- **WHEN** cancellation, source/content replacement or a newer intent supersedes it during the read
- **THEN** its obsolete result cannot publish or borrow another PAD's acknowledgement
- **AND** actual readers and pending promises retire only after their real use ends

#### Scenario: A source window moves while a real read is held
- **GIVEN** the original content has promised its complete source/component bytes, owner witnesses, reader locators and source history before I/O
- **WHEN** its physical placement moves through a pending control mirror during the read
- **THEN** its immutable history promise follows the moved custody row
- **AND** Copy retains independent target history, while a retired row or equal-PCM replacement cannot inherit the original promise
- **AND** final publication still validates the original content lifetime, source/window, proposal and intent

#### Scenario: Historical views end during source preparation
- **GIVEN** a worker explicitly pins historical PCM for its preparation snapshot
- **WHEN** the original resident views retire while that worker still holds the PCM
- **THEN** those actual bytes remain charged once to its own resident scope until its accounting pin ends
- **AND** a foreign scope's PCM reference alone does not revive an ended local owner
- **AND** complete legacy conversion promises its separate transient host envelope before any source read

#### Scenario: Registration or binding preflight waits outside Python
- **GIVEN** a public registration, residency request or current-binding lookup has captured owned shared state
- **WHEN** native reconciliation or registration preflight waits
- **THEN** it retains no Python Engine borrow or interpreter execution
- **AND** registration revalidates current source/window, placement, discard and the original lane lifecycle after its final lock waits
- **AND** only successful registration advances its order watermark, so failed newer work leaves an earlier viable registration intact

### Requirement: Native Conversion Preserves Artifact Semantics
Optimized conversion SHALL preserve bounded streaming geometry, established
binary64/PCM16 rounding and clipping order, RIFF/RF64 handling, cancellation and
exclusive private output custody. Changed processing implementations SHALL have
coherent provenance. Complete immutable artifact verification and descriptor-last
eligibility SHALL remain prerequisites to selection and independent live adoption.

#### Scenario: Conversion is cancelled or fails
- **GIVEN** detached conversion is writing a private generation
- **WHEN** cancellation, malformed input or an allocation/write failure occurs
- **THEN** no incomplete artifact is advertised as ready or adopted by audio
- **AND** only proven owned private output is eligible for failure cleanup

### Requirement: Invalidated Queued Work Retires Its Original Captures
The existing background owners SHALL retire an invalidated unclaimed operation
outside the queue lock and off the audio callback. Its captured readers SHALL
end before its reservation can be reused. Claimed work SHALL retain custody
through actual reader retirement. A refused or unconfirmed replacement SHALL
preserve the previous owner's authority.

#### Scenario: A pending load, analysis or resident preparation is invalidated
- **GIVEN** accepted queued work still owns source or component readers and its payload has not been claimed
- **WHEN** the ordinary control path confirms cancellation or exact source or launch revocation
- **THEN** the background owner retires that operation's captures before returning its admission
- **AND** current or independently shared work and any pending placement keep their own authority
- **AND** retirement does not publish an acknowledgement for a different operation
