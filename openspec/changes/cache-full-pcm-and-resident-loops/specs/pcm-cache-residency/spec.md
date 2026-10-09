## ADDED Requirements

### Requirement: Immutable copy-first decoder input
The system SHALL copy actual stable source bytes into a project-owned immutable
snapshot and hash those bytes while copying before decoding that same snapshot.

The byte-exact project original SHALL remain separate from regenerable PCM.
Content association MUST NOT rely on a path, size/mtime tuple or a pre-decode hash
of another file opening. Snapshot stability and reader lifetime SHALL be enforced;
an input that cannot be captured stably SHALL fail without publishing source state.

#### Scenario: External path changes between copy and decode
- **WHEN** an external path is replaced after immutable snapshot capture
- **THEN** decode and original digest refer to the captured bytes
- **AND** no decoder read reopens the changed external path

#### Scenario: Concurrent mutation or ABA cannot establish false lineage
- **WHEN** mutation prevents a stable capture or a path changes away and back
- **THEN** a stat-only or before/after path-hash match cannot authorize publication
- **AND** only a stable snapshot and actual copied digest can establish lineage

### Requirement: Complete versioned PCM identity
The system SHALL store complete decoder PCM and any complete playback derivative
with versioned identities binding actual original and full PCM content digests,
decoder/processing versions, sample format/layout, rate, channels, full frame
extent, source origin and the executed transform.

Complete interleaved PCM and the complete arithmetic-channel-mean analysis digest
SHALL remain distinct identities. Window hashes MUST NOT substitute for full hashes.

#### Scenario: Identical bytes have a different interpretation
- **WHEN** format, rate, channel layout, extent, origin or processing version differs
- **THEN** the entry is incompatible even if some PCM bytes or paths match
- **AND** the required compatible full artifact is prepared off-thread

### Requirement: Separate rate and transform domains
The system SHALL retain decoder/source, playback, device and analyzer rates and
extents separately without requiring all sources to become 48000 Hz.

Playback derivatives SHALL preserve the existing engine-format contract. Decoder
delay/padding and resampler phase, leading delay, tail and exact ceiling-derived
length SHALL have recorded versioned policies and independent execution evidence.
No unproved trimming or delay compensation SHALL silently redefine source zero.

#### Scenario: 44100-Hz source on a 96000-Hz device
- **WHEN** the source is cached and prepared for playback or analysis
- **THEN** decoder PCM retains its actual 44100-Hz dimensions
- **AND** playback/device/analyzer derivatives bind their own actual dimensions,
  digests and transforms without relabelling accepted historical PCM evidence

### Requirement: Validated complete cache reuse
The system SHALL reuse only complete compatible artifacts whose full source and
PCM content, manifest, exact lengths and dimensions were validated on the
immutable lease used by the consumers.

Fresh warm leases SHALL perform complete integrity verification; verified
immutable in-process leases MAY be shared. Partial, corrupt or incompatible
entries SHALL be rejected and regenerated with bounded work. Integrity I/O and
CPU cost SHALL be accounted for in warm and save measurements.

#### Scenario: Same-size cache corruption or unfinished write
- **WHEN** PCM is changed without a size change or an entry lacks its committed manifest
- **THEN** it is not treated as ready or reused
- **AND** regeneration cannot expose a partially written artifact

#### Scenario: Warm reuse preserves actual decoder and device domains
- **WHEN** a fresh lease requests an existing complete cache for a device format
- **THEN** the complete original, decoder and playback bytes and canonical manifest are verified
- **AND** only the current decoder/processing policy and requested playback rate/channels can authorize reuse
- **AND** an incompatible candidate is regenerated without overwriting any reader-owned generation

#### Scenario: Two subscribers prepare identical content
- **WHEN** two admitted requests have identical stable source content and compatible transforms
- **THEN** preparation shares the compatible digest artifact under serialized admission
- **AND** each subscriber retains its own original assignment and native request/intent/ACK guards
- **AND** cancellation of one subscriber does not cancel or retire the other's preparation or files

### Requirement: Cancellable concurrent cache validation
The system SHALL wait cancellably for a busy cache candidate's validation
admission outside the realtime callback, then perform all complete integrity
checks before reuse. Transient contention MUST NOT trigger redundant cold decoding
or a new cache generation. Cancelling the waiting subscriber SHALL preserve
other subscribers' ownership.

#### Scenario: Concurrent candidate validation preserves warm eligibility
- **GIVEN** a complete compatible cache whose validation admission is held by another request
- **WHEN** a worker searches for its compatible cache
- **THEN** it waits cancellably for that admission and validates the same complete generation after release
- **AND** transient contention does not trigger redundant cold decoding or a new cache generation
- **AND** cancelling the waiting subscriber preserves the peer's cache and ownership

### Requirement: Bounded preparation and guarded publication
The system SHALL bound admission, queue size, worker concurrency and transient
PCM bytes for copy/decode/validation/resampling/window preparation.

Artifact publication SHALL commit an immutable complete entry atomically.
Pad publication SHALL separately check current source/request/intent and window
revision and atomically publish matching handles, metadata and completion state.
Queue-full, preparation failure and pre-adoption cancellation MUST NOT create
partial native ownership.

#### Scenario: Late completion after unload or a newer request
- **WHEN** a worker completes for an invalidated request or window revision
- **THEN** no source, window, analysis, stem or readiness state is republished
- **AND** its exclusively owned temporary artifacts and readers retire off-thread

#### Scenario: Cold adoption waits for native capacity
- **GIVEN** a complete committed cold artifact and the previous effective source
- **WHEN** retirement or feedback capacity delays the native command
- **THEN** the previous audio and project metadata remain effective
- **AND** only matching native adoption ACK authorizes cold Success

#### Scenario: Pending cold adoption is invalidated
- **WHEN** unload or a newer source/timing intent invalidates a waiting cold transaction
- **THEN** the native guard rejects the stale transaction before stopping old audio
- **AND** rollback restores only that transaction's still-current proposed source
- **AND** a newer owner or Manual/Tap intent is preserved

#### Scenario: A backend stalls after claiming native adoption
- **WHEN** the claimed callback tail does not confirm adoption before its finite deadline or shutdown
- **THEN** the system SHALL report unconfirmed native adoption without Success or unsafe rollback
- **AND** complete committed files SHALL remain durable and sealed while still current
- **AND** the proposed source SHALL be fenced from new starts until unload/restart or fresh successful assignment

### Requirement: Full metadata is independent of residency
The system SHALL retain stable complete-source identity, source zero, full rate/
channels/frame count/duration and timing/label provenance independently of
resident ranges, resident frame counts and monotonically guarded window revisions.

Absolute source addresses SHALL be translated explicitly into resident storage.
Changing residency MUST NOT create new accepted evidence, relabel pinned voices,
move loop markers or derive full duration from a window's allocation.

#### Scenario: Saved middle loop has fewer resident than full frames
- **WHEN** only the admitted loop and proved context are resident
- **THEN** full duration and source-relative waveform/grid/seek coordinates remain unchanged
- **AND** CURRENT, MIDI, stems and history refer to matching complete-source and window ownership

#### Scenario: Another assignment reuses identical complete bytes
- **WHEN** a new source assignment reuses a verified compatible full PCM cache
- **THEN** its native assignment identity is fresh even if complete content and backing PCM are shared
- **AND** a storage-only window replacement of that assignment retains its complete identity and accepted evidence
- **AND** a previous pinned voice cannot inherit the new assignment's timing or loop geometry

#### Scenario: Complete evidence is verified from finite playback
- **WHEN** timing restoration or export needs complete PCM while playback holds a finite window
- **THEN** a bounded non-realtime reader verifies the same immutable complete source
- **AND** the accepted ticket retains the finite reference rather than a hidden complete playback allocation
- **AND** complete hashes, frame counts and source zero are unchanged

### Requirement: Proved loop and DSP context
The system SHALL admit a loop window only with independently proved complete-
buffer parity for the executed interpolation, bounds, fractional wrap, rate/
smoothing, Key Lock history/native/FIFO/filter state and stem-transition read set.
Unproved or over-budget context SHALL use an admitted full-track exception or
report preparation unavailable.

#### Scenario: Fractional seam or Key Lock needs samples outside physical loop
- **WHEN** the required context exceeds the proposed loop window
- **THEN** a guessed fixed margin cannot authorize playback
- **AND** the system prepares proved context or a complete-track fallback before readiness

#### Scenario: Normal interpolation and rate changes need no guard PCM
- **WHEN** Normal loop playback uses the admitted physical interval and compatible musical period
- **THEN** both executed taps including the fractional seam remain within that absolute interval
- **AND** changing the source sampling rate or its smoothing does not invent a source lookbehind margin
- **AND** native DSP contexts without independently proved finite continuation use a labelled complete-track exception

#### Scenario: A control requests unprepared context
- **WHEN** requested loop, seek or DSP context is outside the effective resident read set
- **THEN** the request cannot replace effective audio with missing-sample silence
- **AND** it reports unavailable until matching context is admitted or retains prior effective audio during finite preparation

### Requirement: Finite transactional readiness
The system SHALL expose requested intent, preparation/pending/error state and
effective acknowledged source/window state separately for finite replacements.

Existing audio and its complete matching loop/timing/stem/DSP state SHALL remain
valid until the new transaction passes bounded native guards and adoption ACK.
Pending work SHALL NOT claim an effective seek, loop, ALL or accepted timing change.

#### Scenario: Rapid edits while a window is preparing
- **WHEN** a newer edit supersedes a pending window
- **THEN** the previous effective audio continues and the older completion is rejected
- **AND** only the latest matching ready transaction can change effective playback

#### Scenario: Saved geometry is applied again during initial preparation
- **WHEN** startup applies the same saved physical loop and DSP context already captured for finite loading
- **THEN** that identical intent does not invalidate its own preparation
- **AND** a different admitted loop or DSP intent cancels that preparation through native adoption

### Requirement: Shared bounded control transactions
The system SHALL share one source-bound transaction behavior across UI, MIDI
fallback and controls for preparation, cancellation, errors and adoption freshness.
Admission retries SHALL be finite; unload/shutdown or newer source/timing intent
SHALL revoke older continuations. A claimed native tail SHALL complete or report
unconfirmed state before a dependent intent is treated as adopted.

Unchanged start context SHALL reuse matching immutable resident full-mix and
accepted component PCM without cold-worker admission or complete stem file
reload/alignment/hashing, while still requiring matching native transaction ACK and
guarded launch. Repeated starts for the same pending source owner and requested
context SHALL retain that transaction, bounded retry/deadline state and the latest
gesture's original input timestamp without cancelling or restarting preparation.

#### Scenario: An unchanged ready stem start meets a full cold lane
- **GIVEN** a pad owns an acknowledged immutable source/window and matching accepted complete StemSet
- **AND** both cold workers and all queued/reserved job capacity are occupied
- **WHEN** UI or fallback control requests a start with the same resident range and DSP context
- **THEN** the transaction reuses the same full-mix/component PCM and window revision without cold admission or stem reload/alignment/hashing
- **AND** playback waits for that transaction's actual native ACK and retains the original input timestamp
- **AND** queued or scheduled execution rechecks source/window, request, timing authority, latest intent and launch cancellation before playback or exclusive stops

#### Scenario: Identical starts arrive before preparation is acknowledged
- **WHEN** repeated start gestures request the same pending source owner and loop/Key Lock context before native ACK
- **THEN** the existing preparation and ticket remain current without cancellation, restart or supersession
- **AND** only the latest waiting launch action and its original input timestamp replace the prior launch intent
- **AND** retry count and deadline remain bounded without resetting on each gesture
- **AND** matching native ACK and current guarded ownership are still required, and STOP revokes the waiting launch

#### Scenario: A start requires different storage or processing context
- **WHEN** a start requires a different resident range or DSP context, or a replacement source retires the old owner
- **THEN** a prior ready ticket cannot authorize the changed request
- **AND** the new source/context passes applicable load or bounded preparation and native ACK before guarded launch
- **AND** required preparation keeps its capacity-before-supersession admission and preserves previous effective audio until adoption

#### Scenario: A complete stem replacement retires a waiting start
- **WHEN** a replacement complete StemSet is admitted while an older resident start waits for ACK or launch
- **THEN** native pending-owner registration and retirement of the old start authority occur under the source/producer/ownership fence before publication is enqueued
- **AND** a later click cannot coalesce the retired ticket and waits through bounded publication retries for fresh matching ACK
- **AND** reconciliation preserves actual adopted geometry without restoring the replaced component owner
- **AND** rejected queue-full admission preserves the previous owner and start authority, while mode/mask edits alone do not retire readiness

#### Scenario: Queue pressure while a latest edit waits for a claimed predecessor
- **WHEN** bounded admission cannot prepare the latest edit before its retry limit or deadline
- **THEN** previous acknowledged audio and source/timing/stem ownership remain valid
- **AND** error/cancellation restores only the still-current request's prior acknowledged intent
- **AND** a late ACK cannot launch audio or update intent after unload or same-pad source reuse

#### Scenario: Guarded MIDI needs a nonresident loop
- **WHEN** a direct MIDI launch cannot admit the current requested context
- **THEN** its fallback uses the same finite preparation as a UI launch and retains the original input timestamp
- **AND** it launches only after matching native ACK and a fresh native current-source guard

#### Scenario: A prepared global start meets queue pressure
- **WHEN** a complete requested GLOBAL START batch waits for resident readiness or native queue capacity
- **THEN** retries remain bounded and preserve one original input timestamp and the complete target set
- **AND** every retry rechecks current source, timing and restore revision
- **AND** a newer global gesture supersedes the older timestamp, and terminal launch error remains visible after an unrelated region ACK

#### Scenario: A prepared UI start waits in the native schedule
- **WHEN** an acknowledged UI start executes after its original input timestamp was queued
- **THEN** its opaque complete source, resident window, timing authority and latest intent are rechecked before playback or exclusive stops
- **AND** stale source/window or cancelled ownership cannot launch, while an unrelated MIDI runtime refresh preserves the matching UI launch

#### Scenario: A new bank is prepared while a prior-source finite voice plays
- **WHEN** loop preparation retains the effective Key Lock mode after direct bank replacement
- **THEN** the new bank may acknowledge readiness while the prior voice keeps its frozen source, loop and stems until guarded retrigger
- **AND** changing Key Lock without admitted context for that unmatched finite voice reports unavailable

### Requirement: Stop revokes queued starts while preserving resident readiness
The system SHALL revoke prior queued and scheduled UI, MIDI and global starts
when STOP is admitted, preserving acknowledged source/window readiness.
STOP SHALL capture retained targets and revoke launch revisions under the same
native producer admission fence. Global STOP SHALL queue a guarded stop behind
any claimed START tail before retiring its targets. Later start gestures SHALL
use a fresh launch revision.

#### Scenario: STOP follows preparation ACK before playback feedback
- **WHEN** a UI or MIDI start has been queued after resident ACK but no active voice is yet visible
- **THEN** STOP revokes its scheduled launch while the adopted window and CURRENT authority stay valid
- **AND** cancellation of an older ticket cannot revoke a newer ticket's launch

#### Scenario: A global START has passed its final callback guard
- **WHEN** GLOBAL STOP arrives before that START's active feedback is observed
- **THEN** the stop target set includes the pending start targets and a guarded STOP follows its claimed tail
- **AND** starts admitted after the ordered STOP use a fresh revision and can execute

#### Scenario: A direct MIDI start is not yet visible to Python
- **WHEN** STOP arrives after native direct MIDI admission and before input or active feedback is observed
- **THEN** bounded native admitted-start records supply the target for an ordered STOP even after its final start guard
- **AND** a STOP target set larger than voice capacity remains bounded by pad capacity without expanding the START voice limit

#### Scenario: A direct start races with STOP target capture
- **WHEN** a direct MIDI start enters before STOP acquires the shared producer admission fence
- **THEN** the same fenced cancellation returns its retained target even if that start passed its callback guard
- **AND** STOP does not rely on an earlier independent scan or active feedback

### Requirement: Shutdown closes background and mapped launch admission before joins
The system SHALL disable mapped MIDI admission and revoke starts before joining
background work. It SHALL recheck previously captured mappings under the producer
admission fence and close cold-lane admission before cancellation releases active
workers, without changing the saved input preference.

#### Scenario: Shutdown follows captured MIDI mapping
- **WHEN** MIDI mapping was captured before shutdown disables input admission
- **THEN** admission rechecks the enabled state under the producer fence before queueing that mapping
- **AND** shutdown disables MIDI before worker joins without changing the saved input preference

#### Scenario: A cold job remains queued at shutdown
- **WHEN** both cold workers are active and another job is queued when shutdown begins
- **THEN** shutdown closes lane admission and retires queued ownership before cancelling the active jobs
- **AND** the queued job never starts, while active workers drain and their source artifacts retire after final owners release

### Requirement: Complete editor and analysis access
The system SHALL preserve full-source waveform navigation, extreme sample zoom,
source-relative overlays and complete-track analysis through non-realtime
complete-source readers independent of playback residency.

View-only navigation SHALL NOT seek audio. Pending full-source reads SHALL keep
the UI usable, retain honest source-bound readiness and never use a cropped loop
as complete input to timing, key or stem analysis.

#### Scenario: View jumps outside the resident loop
- **WHEN** the editor jumps to the full track's start or end
- **THEN** matching complete-source waveform data becomes available off-thread
- **AND** audio progression, source zero, timing and labels remain unchanged

### Requirement: Bounded complete-source reader ownership
The system SHALL bind complete-reader admission to the full immutable playback
descriptor, actual native generation and sealed reader through actual read return.
Viewport projection SHALL retain only bounded render results and share bounded
worker admission; complete analysis SHALL preserve its existing admission budget.

#### Scenario: Superseded view or analysis after same-path source replacement
- **WHEN** an admitted complete reader finishes after a new view or source assignment
- **THEN** it cannot publish an obsolete waveform or analyze a cropped window as a complete source
- **AND** its readers retire after actual work returns without pinning complete playback PCM

### Requirement: Explicit seek and full-track exceptions
The system SHALL preserve explicit seek inside, before and after the loop:
intro playback reaches the loop, tail playback reaches the actual full source
end then wraps, paused seek remains paused and stopped seek remains a no-op.

Nonresident seeks and ALL SHALL prepare an admitted complete-track resident
exception before effective publication. These are finite preparations and
MUST NOT introduce callback file reads or a continuous streaming framework.

#### Scenario: Seek past a short resident loop
- **WHEN** an active pad requests a nonresident tail position
- **THEN** old playback remains effective during complete-track preparation
- **AND** after matching ACK the tail plays to the full source end then wraps into
  the existing loop without changing markers, grid or auto-loop intent

#### Scenario: A prior finite voice seeks after bank replacement
- **WHEN** a live or paused voice still owns an older finite source while its pad bank has been replaced
- **THEN** seek preparation captures that actual voice generation, source/window and timing
- **AND** it prepares the older sealed complete source and identical already accepted complete StemSet
- **AND** matching native ACK changes only that voice reader and seek, preserving its old stem selection/transition and the new bank's window, stems and timing
- **AND** stop, retrigger, an intervening seek or new source/authority rejects the stale captured transaction before adoption

### Requirement: Same-source stem windows
The system SHALL transactionally adopt full-mix and component stems only with
matching complete-source/accepted-timing identity, absolute range and window
revision. Full extent/channel metadata SHALL remain separate from resident counts.
Stale permits/windows and incomplete sets SHALL remain unavailable.

#### Scenario: Active relocation or new stem generation
- **WHEN** a stem window is relocated while playback is active
- **THEN** the complete source and already accepted complete StemSet SHALL remain identical
- **AND** generation/adoption of a new complete set SHALL require an inactive pad
- **AND** cached instrumental data SHALL NOT become a fifth live component

#### Scenario: Stem result races a window edit
- **WHEN** same-source stems finish for an older window revision
- **THEN** they cannot replace the matching current resident set
- **AND** the prior valid full-mix/stem trajectory and mask continue

#### Scenario: Active storage relocation preserves actual readers
- **WHEN** a replacement covers the live loop and preserves the identical accepted complete source and StemSet
- **THEN** one native adoption transaction changes full mix and components together
- **AND** source fraction, rate smoothing, filter state and actual native/FIFO continuation remain coherent
- **AND** old PCM allocations remain owned by queued jobs or readers until off-thread final retirement

### Requirement: Last-user cleanup and external-original safety
The system SHALL invalidate pending work and retire pad, job, editor/analysis,
queued-command, native-history and pinned-voice readers through bounded off-thread
cleanup before owned-artifact deletion. Shared entries SHALL remain until their final
assignment/reader retires. Cleanup SHALL check containment, serialize reader admission,
retry deferred deletion and tolerate missing files. External originals MUST NOT
be deleted.

#### Scenario: One of two pads sharing a digest unloads
- **WHEN** the first pad unloads while the second pad or a voice/job retains a reader
- **THEN** the shared PCM and required project original remain available
- **AND** final deletion occurs only after the last owner and reader safely retire

#### Scenario: One exact-original assignment survives another pad unload and shutdown
- **WHEN** one pad unloads while another saved project assignment retains the same exact original path and the engine later shuts down normally
- **THEN** removal of the first token does not schedule path retirement and the surviving saved original/cache remain durable after shutdown
- **AND** a stale delivered Success for that still-assigned path cannot schedule its deletion
- **AND** explicit removal of the final project assignment still requests safe last-reader cleanup

#### Scenario: Fresh-process warm publication fails for a saved original
- **WHEN** a saved original has a surviving assignment and a fresh process validates its preexisting complete cache but publication fails
- **THEN** rollback revokes only the failed attempt and cannot acquire deletion rights over that preexisting cache
- **AND** a bounded descriptor without dead PCM backing preserves the validated original-to-cache relation for later explicit last-owner cleanup
- **AND** an already retired descriptor cannot replace a valid durable assignment ID

#### Scenario: Native ownership outlives control assignment
- **WHEN** unload, replacement or shutdown releases a control assignment while a queued handle, job, native history or pinned voice still owns its source
- **THEN** its verified immutable files remain retained until that final reader retires off-thread
- **AND** a claimed but unconfirmed adoption is reconciled without reporting Success or rolling back possibly effective audio

#### Scenario: Repeated restore and ordinary shutdown retain bounded saved ownership
- **WHEN** saved originals sharing a cache repeatedly restore and shut down without explicit unload
- **THEN** bounded file-only descriptors preserve their durable cache relation without retaining dead PCM backing storage
- **AND** explicit last-owner retirement can still remove the cache without unreachable assignment IDs

#### Scenario: Saved assignment reacquires before a deferred retirement retries
- **WHEN** retirement admission was deferred and the same saved assignment acquires ownership again
- **THEN** its older deferred request is revoked before retry admission
- **AND** ordinary shutdown preserves the reassigned original and cache

#### Scenario: An old stem generation retires after a newer set exists
- **WHEN** deferred cleanup of an old generation runs after a newer stem generation is admitted
- **THEN** only the old exclusively owned generation can be removed
- **AND** the newer generation and shared pad container remain intact

#### Scenario: Sharing violation defers deletion
- **WHEN** Windows prevents deletion of a safely retired owned artifact
- **THEN** bounded off-thread cleanup retries while preserving exclusive admission and ownership checks
- **AND** active readers, external originals and unknown files remain untouched

#### Scenario: Recovery encounters crash staging
- **WHEN** cache admission inspects leftover staging
- **THEN** only recognized exclusively owned unreferenced staging can be reclaimed under the cleanup/admission gate
- **AND** live process staging, unknown files and private data remain untouched

### Requirement: Realtime residency boundary
The system SHALL perform disk access, integrity scans, decoding, resampling, JSON,
large allocation/deallocation and deletion outside the audio callback.

The callback SHALL use only prebuilt resident immutable handles, bounded read/
guard/ACK operations and reserved retirement capacity, without GIL, blocking
locks, logging, neural inference, plugin work or unbounded preparation.

#### Scenario: Retirement capacity is temporarily exhausted
- **WHEN** a replacement cannot reserve safe retirement and feedback capacity
- **THEN** effective publication is deferred without freeing payloads or reading disk on the callback

### Requirement: Measured cache and residency acceptance
The system SHALL establish cache/residency claims through real cold/warm 200-pad,
short-loop/long-source and explicit full-track measurements with source identities,
resource/integrity costs, lifecycle and independent timing/playback parity.
Human listening/device acceptance SHALL remain separately open until its final
human-run stage.

#### Scenario: Resource and parity evidence is gathered
- **WHEN** cache/residency measurements establish a claim
- **THEN** evidence SHALL include readiness, worker/handle peaks, steady/transient/process RAM and disk/integrity/save I/O/CPU
- **AND** corruption/cancellation/cleanup and 44100/48000/96000-Hz parity SHALL be proved
- **AND** fractional periods/rates/starts/tails, Key Lock/stems and accepted timing SHALL be checked
- **AND** unwrapped loop bounds SHALL remain within one loaded frame

#### Scenario: A warm startup report claims improvement
- **WHEN** cold/warm results are compared
- **THEN** sources and conditions match and integrity/full-track exception costs are reported
- **AND** current 96-native-handle costs are measured rather than inferred from
  historical 64-handle results or substituted for hearing/device acceptance
