# Native accepted timing adoption

G3b2a-i connect the G3a accepted record to actual loaded-pad ownership, current
acknowledged authority, native SourceGrid, transport/output clock and playback
rate. This is an explicit control API. Normal loading and analysis,
manual/TAP controllers and saved legacy projects retain their existing routing;
their numerical values are not promoted to accepted evidence.

## Explicit preparation and acceptance

`AudioEngine.set_pad_timing_intent(sample_id, intent)` accepts `automatic`, `manual`,
`tap` or `legacy`. Automatic preparation requires explicit Automatic intent.
Returning to that intent after another edit cannot revive a prior ticket.

`prepare_constant_timing(sample_id, timing_error_halfwidth_seconds,
timing_error_provenance)` captures the actual loaded source and current native
request, digest, generation and loaded rate. It creates verified complete
channel-mean PCM and executes the shared native lossless QM pipeline off the audio
callback. Its opaque `ConstantTimingTicket` cannot be constructed from Python
metadata or persisted. `metadata()` exposes source/PCM/job/timebase and complete
raw evidence for independent count assessment. Preparation does not accept a
musical interpretation or change the default estimator.

`publish_constant_timing(ticket, hypotheses_json, origin_seconds,
origin_provenance, acceptance_policy_version, acceptance_provenance)` recomputes
the supported raw fit and constructs `AcceptedConstantTiming`. The hypotheses
JSON is a list of objects with `id`, `provenance`, `verification` (`verified` or
`unverified`), `quarter_note_denominator` and `quarter_counts` (integer numerators
or null for explicit exclusions). Counts correspond to every original raw event;
the API does not derive them from array indices or insert a missing source-zero
attack. The denominator expresses the chosen musical interpretation explicitly.
The same G3a support, ambiguity and uncertainty rules apply.

Verification and acceptance are explicit caller assertions with named provenance.
Their consistency is checked; their independent musical truth is not established
by a good fit. This API supplies no new musical labels, general refinement policy
or automatic acceptance threshold. Timing-error provenance and selected signed
origin remain separate from the fitted diagnostic intercept and source zero.

## Publication ownership and feedback

Native current source/request/intent ownership is checked after preparation and
again during publication under the request owner. Matching stale caller snapshots
do not establish current-pad validity. The production caller uses
`TimingAdoptionGuard` and retains complete evidence outside realtime processing.
Existing request/epoch ownership invalidates pending work on load, unload, new
analysis requests, cancellation and successful timing edits, including same-value
edits. Explicit Manual, Tap and Legacy prevent automatic publication.

Ordinary Analyze admission also advances the shared request/epoch. Late normal
analysis results cannot replay legacy timing after a newer request. A still-current
source's load completion retains path/duration/loading bookkeeping while marking
retired timing with `timing_stale`; Python then skips old analysis, automatic grid/
loop initialization and legacy restore replay. Retired analysis completions settle
their matching task without clearing a newer task. Replaced-source events are
discarded and polling continues past retired events. Preparation rejects pads
that are still loading or have active native tasks.

Checked counters and queue reservations precede state mutation. Full rings and
invalid/stale inputs retain previously effective timing. The callback uses bounded
source-pointer and atomic-permit checks. Its payload retains exact binary64
period/origin and fixed bytes of the complete accepted revision. Large sample
pins and evidence retire outside the callback through existing retirement paths.
Earlier queued legacy parameter updates cannot erase a newer accepted projection;
a successful later legacy edit revokes it.

`ticket.publication_status()` distinguishes `captured`, `pending`, `accepted` and
`rejected`. Enqueue is pending, not effective availability.
`ticket.accepted_metadata()` becomes available only after actual mixer acceptance.
It describes that ticket's historical adoption; it is not a current-pad polling
authority after later source or timing edits. Polling observes acknowledgement
and never drives audio progression.

`AudioEngine.current_constant_timing(sample_id)` resolves current metadata from
the actual loaded source Arc/digest/generation/rate, Automatic timing authority
and fixed callback acknowledgement. It includes the complete accepted revision,
accepted request identity, original/mono content identity, source zero, exact
period, independent signed origin and acceptance provenance. The native retained
record owns no PCM allocation, including weak array ownership. A non-dereferenced
source address/extent is checked with native monotonic loaded generation, digest
and rate, so recycled addresses cannot substitute for current source ownership.
Historical `ticket.accepted_metadata()` can remain available after this resolver
returns `None`. New requests and pending replacement retain the previous effective
revision until actual adoption. A changing acknowledgement during a lookup returns
unavailable for that poll. Successful Manual/Tap/Legacy edits revoke authority even
while the callback clear waits; returning to Automatic cannot revive old timing.
Full queues and failed edits preserve the previous current result. Retention and
pruning occur outside realtime processing.

The mixer creates SourceGrid directly from the accepted period multiplied by the
actual loaded rate and the independent signed origin. It does not derive the
period through binary32 BPM. Publication leaves source zero, stored physical loop
endpoints and active fractional source progression intact.

## Source verification limits

The complete mono PCM digest verifies the actual loaded sample bytes under the
existing arithmetic-channel-mean rule. Arc, generation and loaded timebase checks
bind that PCM to the current native pad. The original digest remains the loader's
observed file-content association. Decode-before-copy and path replacement/ABA
limits are unchanged: this does not prove an immutable original was the exact
decoder input. Copy-first ownership and a full versioned PCM cache remain C1.
The loaded buffer is resampled/channel-mapped at the existing engine rate;
original, loaded and QM analyzer timebases remain distinct.

## Native shared-period and rate consumers

Native source timing prefers the acknowledged accepted period over legacy BPM.
Transport retains authoritative binary64 output seconds per quarter. Its bootstrap,
phase anchor, beat progression and quantization use that period directly. The shared
OutputClock stores exact period bits; `output_clock_snapshot()` exposes
`master_period_seconds` and derives its compatibility `master_bpm` for presentation.
Legacy public speed, master BPM and pad BPM retain binary64 through messages.

BPMLOCK derives one target as `source_period / master_output_period`; start and
render share that resolver. Global speed applies when unlocked or timing is
unavailable. Supported speed clipping cannot redefine the requested master period.
SourcePlayback rate, target and smoothing preserve binary64 and rebase from the
existing fractional source position. The same cursor supplies full mix and stems.
Rubber Band receives a named inverse-rate conversion through its native `c_double`
pitch ABI. Its existing near-unity and update-threshold behavior remains; this
does not establish sub-threshold pitch application or audible SYNC. Productive
history ownership is described below. Physical integer-loop wrapping is unchanged.

Regression tests exercise actual callback drains and rendering/bootstrap, including
legacy BPM deliberately disagreeing with accepted timing, replacement while playing,
rate clipping, precise long source epochs and a quantization boundary where a
binary32 BPM projection would choose the wrong output frame.

## Python current consumers and global controls

`BpmController.current_timing()` resolves a frozen source period/origin/loaded-rate
snapshot with full accepted revision and current source/adoption identity and
provenance. Manual project intent takes priority, followed by native current
acknowledgement and ordinary Legacy analysis. It never reads historical ticket
metadata. `AudioEngine.pad_timing_intent()` exposes declared authority without
claiming acknowledgement; Automatic with no current record stays unavailable.

Scalar grid, waveform editor lines/view margin/loop-relative labels and loop
snap/clamp/bar duration/maximum-auto/effective endpoints use the exact period.
One operation carries the same resolved snapshot, including unavailable `None`.
Signed accepted origins retain their fractional source position. Physical marker
conversion rounds each absolute boundary once at the actual loaded rate.
ALL, maximum auto-loop length and waveform navigation/query clipping use the
accepted full frame extent rather than stale saved duration. The UI waveform
cache/view key includes native source identity/generation/digests/extent/rate;
timing-only revisions of the same source preserve navigation. This bounded UI
cache binding does not implement the future C0-C3 PCM cache/residency design.

Passive restore/refresh/BPMLOCK activation skip legacy BPM and origin writes for
Automatic authority. Unavailable automatic restore preserves native loop/master
state. Intentional Manual/Tap edits retire acceptance, and final native intent
follows legacy numeric/origin publication. Clearing manual or editing a legacy
grid offset resumes existing Legacy analysis/grid policy without caching accepted
evidence; accepted origin is not copied into saved legacy base intent. Failed
first numeric admission preserves the saved BPM/offset intent. Validated load,
unload/reset and ordinary analysis completions restore Legacy before derived
callbacks; pending or retired timing_stale work cannot revoke current acceptance.

Accepted master control publishes `current_source_period / speed` directly through
`AudioEngine.set_master_period()`. This parameter and compatibility BPM share one
coalesced lane with last-admitted ordering. Session master period/revision records
the last successful publication, while display BPM is derived from period. Speed,
BPM target and nudge reuse one source snapshot; unavailable Automatic anchors
reject explicit speed changes before saved speed or native speed is changed.
`set_speed_and_master_period()` admits an accepted locked speed change as one
bounded parameter effect, so a full ring cannot admit only half the update.

General explicit publication uses the application's `accepted_timing` controller.
It prepares actual native evidence on a bounded worker and accepts only explicitly
supplied musical hypotheses, signed origin and acceptance policy/provenance. Native
capture precedes worker execution, so delayed work cannot capture a newer source.
The explicit Automatic choice remains separate from accepting musical evidence.
Preparation preserves existing publication/refresh observers, including when a
replacement capture fails. A same-pad publication waits for outstanding publication
or derived completion to settle; invalid or stale overlapping assessments cannot
abandon the preceding genuine acknowledgement or restored-stem completion.

After publication acknowledgement, the controller resolves actual CURRENT native
timing and its opaque source/authority binding anew. It calculates the physical
loop through the existing loop evaluator with that frozen exact period/origin/rate
and full identity. A selected BPMLOCK anchor supplies `period / speed` directly.
`refresh_current_constant_timing(binding, start_s, end_s, master_period_seconds)`
admits those derived values as one guarded native effect. It never replays Legacy
BPM or origin, refits evidence, or treats historical ticket metadata as current.

The callback schedules refresh at its start frame plus one, after the existing
bounded parameter drain. Older parameter backlog rejects the whole effect; later
speed/master/BPMLOCK admission invalidates its global control revision. Current
source/authority/full accepted projection and retirement/scheduler capacity are
checked before effects. Loop and optional master/bootstrap commit together;
failure leaves both previous derived values available. This one-frame boundary
orders control lanes and does not introduce a second musical clock.
The coupled master effect replaces an older pending bootstrap reference with its
selected current pad; an already completed one-time bootstrap keeps its epoch.
An older foreign reference becoming ready afterward cannot anchor the beat phase
or consume the selected pad's one-time bootstrap. The exact master period remains
preserved under native BPMLOCK.

An active or paused voice pin belonging to an older bank source cannot acquire
the new source's loop geometry or accepted ownership. Such a refresh rejects as
a whole; stop or restart under the current source permits a new explicit retry.
Master refresh also requires actual native BPMLOCK and exact agreement with the
accepted period divided by current native speed, so a direct control edit between
Python calculation and native admission cannot publish a stale derived master.

`AcceptedTimingRefreshTicket.publication_status()` reports actual execution, and
`is_current()` checks its retained source/authority and applicable global control
revision. Session master period/revision and acceptance completion become visible
only after accepted current refresh and matching application control intent.
Pending, rejected, unavailable or stale completion cannot project provisional
analysis or a historical accepted ticket. Polling observes explicitly requested
work; ordinary read-only UI polling does not publish timing or drive progression.
Admission pressure receives at most three still-current attempts; rejected execution
or exhausted retries reports the failure and leaves an explicit retry available.
Normal estimator/default musical acceptance remains a later gate.

## MIDI current source and authority

`current_input_runtime_pad_binding(sample_id)` captures an opaque native source
and authority snapshot outside the callback. Its metadata names the actual source
generation, digest, loaded rate, full extent and channels, declared timing intent
and authority revision. Its optional `accepted_timing` is resolved through the
same current acknowledged record as `current_constant_timing`, with complete
accepted revision, period, signed origin and provenance. It owns no PCM buffer;
caller metadata cannot construct or persist this binding.

Python carries one frozen `CurrentPadTiming` through effective loop calculation
and signature comparison, checks it against that native binding and publishes the
binding with runtime loop intent. Equal BPM/endpoints do not hide a different
source, full accepted revision or same-value authority edit. An unavailable or
inconsistent Automatic snapshot disables the pad's direct runtime until a fresh
matching publication; it does not replay saved analysis or clear the live loop.

Direct MIDI loop intent and launch are one fixed guarded command. Native refresh
revalidates current ownership before replacing the runtime. Callback/scheduled
execution rechecks source identity, declared authority and acknowledged accepted
projection before applying loop intent or exclusive playback. A newer effective
runtime loop invalidates old queued/scheduled loop intent. Successful source or
timing edits invalidate old bindings immediately, including Manual/Automatic
roundtrips. New preparation requests and pending/rejected replacements retain the
previous effective accepted record. No PCM scans, evidence allocations, locks,
Python or I/O enter the callback.
Scheduler capacity is allocated once on the heap before callback construction;
the larger guarded event does not create a large inline startup stack temporary.
Scheduling and execution reuse that fixed storage without callback allocation.

Failed direct pad-trigger events refresh current runtime and retry through
`trigger_input_runtime_pad`, retaining the captured timestamp and guarded
all-or-nothing semantics. They do not use the ordinary unguarded Python loop/play
sequence. A full queue, unavailable Automatic state or stale binding admits no
partial loop or launch. Enqueue feedback still reports admission rather than
audible or scheduled-execution acceptance; a subsequently retired trigger is
discarded at execution. MIDI global actions now use the controller batch path below.

Runtime publication changes dormant input intent; MIDI polling does not publish
accepted timing, refresh master controls or change a live loop. General explicit
accepted adoption uses the application completion route described above.
Controller GLOBAL START/STOP, including mapped MIDI, reuses this native source/
authority resolver without creating another timing owner.

## Controller GLOBAL START/STOP batches

The productive controller captures one `CurrentPadTiming` and opaque
`InputRuntimePadBinding` per affected pad, then uses the same exact source/adoption
comparison as MIDI runtime publication. Complete accepted identity, period and
signed origin bits must match; equal BPM, endpoints or signed-zero numerical values
cannot substitute for the full binding. START calculates each effective loop from
its captured snapshot. Automatic without current acknowledgement fails admission;
Manual, Tap and Legacy retain nonaccepted authority. New preparation and pending/
rejected replacements retain the previous acknowledged accepted record.

Native START accepts binding/start/end tuples; STOP accepts current bindings for
every active pad, including paused voices. A batch is bounded by voice capacity
and one `GlobalPlaybackBatchTicket` reports `pending`, `accepted` or `rejected`.
START retains one captured Rust input timestamp and schedules one common output
frame through the existing output-clock and quantization path. Enqueue or schedule
success alone does not establish playback acceptance. Immutable batch storage is
allocated on the control path; scheduler capacity is allocated at callback setup.

Native admission and immediate/scheduled execution recheck every actual current
source, declared authority, acknowledgement and complete effective accepted
projection before any effect. Execution validates all voice and off-realtime
retirement and native playback-feedback capacity before applying any loop, start,
stop or transport bootstrap. The entire set of required SampleStarted/SampleStopped
messages must fit the feedback ring before the first audio effect; command-ring
capacity alone cannot protect the controller projection. The callback is its single
producer, while concurrent draining can only free slots.
Starts reuse existing single-pad preparation, canonical `SourcePlayback`, rate
and source/native/FIFO/filter invalidation. A current-bank STOP also checks the
actual active pins: an old voice retained after bank replacement cannot be
relabeled with the replacement's current timing and rejects the entire batch.
An admitted restart adopts current bank PCM and retires old pins through existing
off-realtime paths. No failed subset can stop unrelated voices or rewrite loops.

The controller leaves active/paused truth to native playback messages. It observes
terminal ticket feedback only for GLOBAL restore intent: an accepted STOP remembers
the captured playing subset, and an accepted remembered START consumes that intent.
Generic stop-all creates no remembered restore set. Before each subsequent global
target capture, the controller drains native playback feedback through the existing
app event handler after observing any accepted ticket. A following MIDI/keyboard
STOP therefore sees starts whose playback feedback has not reached the normal
frame poll yet. Native unload reserves one feedback slot before execution and
publishes an ordered SampleStopped; full feedback defers the whole unload, so
queued older START feedback cannot leave an unloaded pad active in the controller.
Actual SampleStarted telemetry clears the started pad's paused projection.
A loader unload/replacement callback
prunes that pad from observed and pending restore ids and normalizes restore
engagement; a late accepted STOP cannot put it back, while unrelated captured ids
remain. Pending or rejected admission/execution retains prior restore bookkeeping.
Full command/feedback/scheduler rings,
unavailable timing, stale binding, exhausted voice or retirement capacity preserve
the prior effective audio, loops, transport and ownership. Mapped MIDI global
actions dispatch through this same controller transaction with their original
input timestamp; direct unguarded MIDI stop-all no longer bypasses it.

Callback work uses fixed-capacity scalar/atomic checks, scheduler storage and
feedback. PCM hashing/scanning, evidence allocation, locks, Python/GIL/UI, I/O,
logging, native DSP construction/reset and large-owner destruction remain outside
it. Batch ticket polling cannot drive audio timing or accepted publication.
General explicit publication and derived loop/master completion use the separate
application route described above, sharing the current binding and native guards.

## Prepared source and stem timing binding

`capture_prepared_source()` uses the current native source/authority resolver to
capture a fixed binding alongside the existing source/content/request/rate/epoch
owner. Automatic requires a consistent current acknowledged complete accepted
revision, exact period and signed origin; unavailable Automatic fails admission.
Manual, Tap and Legacy keep their own authority. Historical ticket metadata,
equal endpoints, raw revisions, source hash or preparation epoch alone do not
prove current accepted ownership.

Source generation and digest are verified by the shared resolver at capture.
The permit's fixed InputPadBinding carries source address/shape/rate, monotonic
authority revision and full accepted projection with atomic owners; it does not
retain separate generation/digest fields in the callback. The pinned source and
matching source_version digest plus request/epoch/authority guards preserve ownership.

The productive preparation path validates that binding before and after off-thread
decoding/alignment and through enqueue. PreparedStemSet carries the fixed binding
to callback adoption, which rechecks actual current source, authority and accepted
projection before replacing audio. A source/timing refresh between these stages
cannot adopt a stale set. Failed admission and late rejection preserve previously
admitted audio; pending/rejected timing replacement never makes its proposal current.

A successful Legacy/Manual/Tap edit revokes control acknowledgement before its
parameter callback clear may execute. A fresh nonaccepted preparation captured in
that interval can be safely rejected against the mixer's still-effective accepted
projection. Previous PCM/audio remains valid; fresh capture after actual clear
can succeed. Control availability is not a promise of callback adoption during
this transition.

Admitted same-source stems retain their immutable PCM when native accepted timing
is successfully adopted or cleared. The mixer updates only their fixed effective
projection and source_reader checks it exactly against effective mixer timing.
Pending/rejected updates do not change that projection. Full mix, stems and their
source-selection transition continue reading one SourcePlayback trajectory with
the existing rate/ramp, fractional epoch, interpolation and loop/seek rules.
No second cursor, PCM rebuild, physical-loop rewrite or playhead reset is needed.
The callback adds bounded fixed/atomic checks and projection copies; large owners
and evidence remain in off-realtime retirement paths. See
[prepared stem ownership](prepared-stem-publication.md).

## Productive voice and native/FIFO history

G3b2f1 covers continuous productive history ownership. `StretchProcessor` fills
its fixed feed directly from the actual borrowed source
through `SourceReadPlan` and the canonical `SourcePlayback`. Actual consumed
Rubber Band history and pending input/output FIFOs carry source address/shape/
loaded rate and the complete effective accepted projection. Period and signed
origin match exact binary64 bits, including signed zero. A warmed reserve remains
source-neutral; it is not source-specific history or priming.

Before consumption, expected next fractional source position and seek mode are
checked against the canonical cursor. Source replacement or discontinuity clears
bounded adapter storage and marks used native state dirty before foreign feed
can enter old state. Native construction/reset/warming use the existing worker;
callback exchange reserves its bounded return lane before ownership moves.

Continuous same-source accepted adoption/clear updates the complete effective
projection on productive feed while retaining chronological native/FIFO history.
Equal timing numbers do not hide a different full revision. Rate smoothing keeps
the fractional epoch; pause freezes feed and history; stem selection crossfades
retain state. Start/retrigger, stop, seek, wet deactivation and discontinuous loop
clamps invalidate history using existing bounded rules. Pending, rejected or
failed timing does not change its effective binding.

An active `VoiceSlot` owns the sample it actually pins and that source's effective
timing. A replaced bank's current accepted record cannot relabel old voice PCM.
That voice can continue its previous source/timing until retrigger or stop;
its retained ownership is not current pad-bank acceptance. Retrigger adopts the
current bank PCM and retires the old sample through existing off-realtime paths.
Successful same-source adoption/clear refreshes only matching voices. Failed
admission or late rejection preserves the previously effective audio/history.

An explicit active seek uses that voice's pinned source extent rather than a
shorter replacement bank. Every successful seek, including the same position,
clears bounded adapter/FIFO history and fixed per-pad filter ownership. Native
reset/warming remains on the preparation worker.

New voice/retrigger adoption rejects unavailable Automatic or an effective
accepted projection whose acknowledgement has retired. It also requires the
effective bank PCM to match native current control-source ownership. Fixed
generation/address/shape/rate atomics provide one bounded check/recheck, without
spinning or retaining PCM/evidence in the marker. Loading revokes that marker
before control cache ownership is removed. Successful native loaded publication
installs queue/cache/generation/digest under the request owner before publishing
the marker; full-queue publication changes none of those values. Unload/run also
revoke it. New starts reject while loading is unavailable or replacement control
PCM is ahead of callback bank adoption, including under Legacy authority.

Existing effective audio/history keeps its own pin and can continue during these
source transitions or while an admitted callback clear waits. Fresh Manual/Tap/Legacy
adoption can proceed after actual clear under its own authority. Replacing a
voice pin reserves existing off-realtime retirement capacity before old voice,
loop or exclusive playback changes. G3b2h reuses these single-voice checks in
the complete guarded global batch described above.

`PerPadDspChain` binds its actual EQ/isolator filter history to the same rendered
voice source/projection and expected fractional next position. Continuous
same-source timing/rate changes retain filter state. A source/loaded-rate mismatch
or discontinuity clears only bounded fixed Rust filter storage before replacement
output enters it. The ledger counts actual filtered output, including existing
wet fallback silence; it does not assert that silent output contains audible
source samples.
Stop/unload clears the filter ledger before the owning voice pin retires;
the ledger owns no additional PCM or native handle.

G3b2f1 supplies continuous productive history binding. G3b2f2 adds the prepared
native continuation below; G3b2g subsequently adds complete source-verified persistence.
These ownership contracts
supply no audible delay/crop/transition guarantee; that remains later B5 work.
Existing neutral-reserve unavailability retains wet silence with canonical source
progression. `key_lock_source_preparation` remains test-only.

## Productive prepared native continuation

`NativeHistoryContext` captures `NativeHistoryPermit` at a productive render
boundary. The permit retains fixed `InputPadBinding` and shared atomic owners:
actual source address/shape/loaded rate, tracked native loaded-request generation,
shared preparation epoch, authority and runtime revisions, and complete effective
accepted projection. Current checks repeat a bounded source/epoch/authority/
runtime/acknowledgement test without spinning. Accepted period and signed origin
compare exact binary64 bits, including signed zero. Automatic without current
acknowledgement cannot capture; Manual, Tap and Legacy remain nonaccepted.
The shared epoch retires preparation after new analysis, cancellation, load and
successful timing edits, including equal-value edits. The voice's local request
counter and shared atomic invalidation epoch separately identify its runtime
trajectory. The worker checks that local epoch before and after catch-up; actual
adoption also requires the exact outstanding request ID.

The existing `key_lock_preparation` worker receives a request pinning the actual
`SampleBuffer` and optional admitted `PreparedStemSet`, plus copied canonical
`SourcePlayback` and `SourceReadPlan`. `prepared_native_history::NativeAdapterState`
owns the actual Rubber Band handle and fixed adapter storage. A third preallocated
per-voice state supplies preparation capacity alongside the effective owner and
neutral warmed reserve. It is not neutral warmed: the worker prepares exact
starting pitch/reset off realtime, then processes 4096 active output frames from
real requested PCM through the same fixed native adapter and fractional reader as
productive rendering. Copied chunk/advance preserves rate smoothing, physical
loop, fractional epoch and explicit seek behavior. Native state and complete
input/output FIFOs remain retained as a coherent continuation with their source/
stem pins; neutral warming alone cannot provide it. Active stem-selection
transitions defer requests until complete while the old effective owner continues.
Current and prepared adapters use setup-allocated `Box<NativeAdapterState>` owners
through ready/recycle/pending-return lanes. Pending-request retention is also
heap-allocated at setup, and request rings have fixed preallocated storage. This
keeps large retained payloads out of the 32-voice Windows stack aggregate; the
callback moves/swaps existing boxes only. Catch-up feed/output scratch stays off RT.

The adoption deadline is the actual request output frame plus 4096.
`StretchProcessor::chunk_until_prepared_adoption` splits rendering at that exact
absolute frame. Adoption rechecks the current permit and complete effective
projection, source/stem pointer identities, read plan, full canonical playback
checkpoint and local request/epoch. It reserves bounded recycling capacity before
exchanging the complete native/FIFO owner. The logical source cursor continues
unchanged; callback preparation/catch-up is prohibited.

Pending, worker-failed, unready, late, stale or recycle-saturated results preserve
previous effective native/FIFO history and output. Rejected results and old
prepared owners return through bounded lanes for off-thread recycling/destruction;
source/stem pins do not undergo final callback drops. Old retained bank-source
voices continue under their own effective source/timing but cannot authorize a
new current-source preparation once tracked source ownership differs. Equal
timing values cannot hide changed complete accepted revision or source generation.
A reset may retire request pins through a separate bounded worker lane, retaining
the invalidated dirty native/FIFO state until safe exchange; its dirty-state fence
prevents foreign feed even after those source owners have retired.
Stop/reset/wet deactivation publish local cancellation before recycling owners.
The worker destroys stale in-flight request pins off realtime and records completed
request IDs so cancellation cannot leave the voice permanently pending. Inactive
and paused voices poll the bounded ready/recycle retirement path every callback,
without reading source audio, to retire a result published after cancellation.
Stream teardown retires remaining owners off realtime after rendering stops.

The 4096-frame horizon bounds preparation work; it does not define native delay,
audible crop, transient alignment, a seamless wet/bypass transition or a device
deadline guarantee. Stream setup now constructs 96 unique native handles for 32
voices, including the 64 existing neutral-warmed handles and 32 source reserves.
The extra owner has startup/memory cost; historical two-handle measurements do not
measure this extension. The productive worker, handle/FIFO, nonzero shifted-output
and failure-path tests below establish numerical G3b2f2 ownership/continuation;
the test-only preparation fixture remains separate evidence.

The production adapter oracle matrix covers 324 steady and 54 smoothed cases:
mono/stereo at 44.1/48/96 kHz, physical loops, intro/tail seeks, full mix, component
stems and ALL. An independent algebraic fractional-source reader feeds raw Rubber
Band without the shared adapter; each case compares a 20,017-frame prepared
continuation bit-exactly and requires genuine shifted samples above 0.02 amplitude.
Actual mixer/worker tests separately verify current/prepared native-address and
FIFO ownership transfer at the deadline, a 12,853-frame productive suffix against
the raw-native oracle, 15 current-source/timing/runtime invalidation cases, active
rate smoothing, deferred/changed stem selection, same-phase seek, late/failed/
recycle-saturated adoption and real worker pin retirement while preparing, ready
or adopted. Inactive retirement polling is exercised without another source read.
The legacy partition fixture isolates continuous playback with preparation
unavailable; productive readiness/adoption has separate tests. These are numerical
ownership/continuation checks, not full G3c or later B5 acoustic acceptance.

## Source-verified accepted project persistence

`SampleAnalysis.accepted_timing` holds a versioned historical envelope. The
supported encoding is `accepted-constant-timing-qm-raw-v1`, schema version 1.
It retains the complete native raw QM/configuration/input-transform/count/error
evidence, full canonical accepted revision, source/mono/rate/full extent/source
zero, and independently declared origin and acceptance provenance. Binary64
values are encoded as exact hexadecimal bits, including signed zero. Other
evidence encodings, including refinement and Beat This, cannot be imported by
this bounded codec. They remain unsupported without promotion or identity loss.

`ProjectPersistence` binds its running native owner. Every actual flush creates
a save snapshot from `export_current_constant_timing(sample_id, source_path)`,
which requires current acknowledgement and rehashes actual project source bytes
and full owned mono/analyzer input off realtime. A standalone save without this
owner, unacknowledged Automatic, or nonaccepted authority omits historical
accepted blobs. Source verification failure aborts the atomic write and preserves
the previous file and dirty state. Debounced and immediate UI save paths report
the pad error without escaping expected verification failures; rejected background
save retries are throttled by the existing debounce interval. Shutdown drains
restoration and tears down native audio even when verification rejects saving.
Automatic remains explicit even when evidence
is unavailable. Saving currently performs integrity work synchronously off the
callback; its I/O/CPU cost has not been benchmarked.

On restore, Automatic is reserved before startup can project old BPM/grid values.
After successful matching async source load,
`capture_saved_constant_timing(sample_id, record_json, source_path)` captures a
fresh native source/request/authority epoch before a single Python worker calls
`restore_constant_timing(ticket)`. It verifies actual source bytes and complete
mono/rate/extent/source zero, reconstructs the supported assessment and canonical
identity, then uses the existing `TimingAdoptionGuard` and native publication.
The historical evidence job and accepted revision stay distinct from new runtime
generation/request ownership. No persisted flag or ticket acknowledgement is
CURRENT until genuine fresh callback adoption. Pending, failed, rejected or stale
work leaves Automatic unavailable. Only matching current native acknowledgement
triggers restored intent completion. In the application, fresh saved adoption uses
the same guarded general loop/master refresh route before stem intent completion.

`ProjectState.pad_timing_intent` durably distinguishes Manual, Tap and Legacy.
A manual BPM override suppresses Automatic restoration; Tap retains its origin
as performer intent across startup. Missing intent in old projects migrates to
Manual for existing overrides and otherwise Legacy. Unsupported envelope shape
is dropped per analysis record without replacing valid legacy metadata or intent.
Ordinary new load/analysis continues Legacy behavior. Original-byte rehashing
preserves the observed native-loader association; it does not establish C1's
immutable copy-first decoder input or defeat an original-file ABA replacement.

## Remaining shared-period and loop work

G3b2f1/f2 supply productive continuous and prepared native/FIFO ownership with the
numerical timed-adoption proof above. G3b2g adds source-verified persistence and
fresh loader adoption of supported COMPLETE native QM raw accepted records;
unsupported evidence is rejected rather than converted to compatible-only timing.
No opaque ticket is a saved identity and no saved Manual/Tap/Legacy BPM is accepted
evidence. G3b2h binds productive controller GLOBAL START/STOP batches, including
MIDI, to current source/authority and actual execution feedback. G3b2i adds general
explicit accepted publication and guarded acknowledged derived loop/master refresh,
shared with fresh saved adoption in the application.
Original hash association still does not prove C1 immutable copy-first/ABA lineage.

G3c remains separate: musical period versus rounded physical loops over 75/1000
cycles, nonintegral periods/rates and callback partitions/wrap. Numerical grid
tests do not pass rendered DSP, onset, device, listening or sustained audible SYNC gates.

G3c1's [loop-period proof](loop-period-proof.md) separately characterizes productive
dry PCM and threshold features against independent oracles. Integer physical
wrapping repeats endpoint-rounding error when the intended musical duration is
fractional. A passing physical oracle is not musical success; the strict musical
probe and shared trajectory/reader correction remain G3c2, followed by actual
device and sustained listening evidence.

See [accepted record identity](accepted-constant-timing.md),
[prepared stem ownership](prepared-stem-publication.md) and
[source coordinates](scalar-source-coordinates.md).
