# Independent pad content copies

The empty-target Copy core copies musical content to a fresh stopped pad. It
shares verified immutable material, source PCM and matching stem backing, while
each assignment owns independent source custody, playback and mutable settings.
Occupied-target overwrite, live Move/Swap, bank operations and Re-Arrange gestures
have separate contracts in
[add-rearrange-shared-pad-content](../openspec/changes/add-rearrange-shared-pad-content/specs/pad-content-arrangement/spec.md).
This core does not close those later gates.

## Internal occupied-target foundation — 2026-10-14

The existing Native Copy transaction can now prepare an occupied target when
`capture_pad_copy` receives that target's exact opaque `target_binding`. Omitting
it still requires an empty target. Captured lifetime, source, timing, physical
placement, preparation, runtime, authority and bounded selection/proposal guards
must remain current. Equal PCM or an overwrite flag cannot authorize replacement.

One existing callback claim retires the original target's Hold effect and voices,
then installs a fresh stopped content. The source keeps its living voice, history,
DSP, cursor and holds. Displaced bank/stem/receipt handles and voice readers leave
through the existing off-realtime retirement lane. Fixed guards and conservative
retirement capacity bound callback work. Pending or pressure retains the exact
payload; a stale target or preclaim cancellation rejects without replacing either
participant. The own Native acknowledgement controls replacement of the target's
control cache, including removal of an old selected set when the source is FullMix
without selected stems.

The internal `PadArrangementController.copy_overwrite` captures the original
musical target seal and fences target observers until settlement. Only after the
own complete Native acknowledgement does it retire original controller tickets
and that target's job subscriber. Other subscribers and actual readers survive.
The occupied config path retains one prepared `AssignmentPublicationPlan` through
interrupted table transfer and old-owner retirement; retries settle its original
cursor before incorporating newer unrelated intent. Generic cancellation after an
actual disk-selection replacement does not revoke the committed selection.

A Pair-delayed duplicate generation retains its original source/model/subscriber
owners. It may reuse a ready completed set, transfer to an actually active canonical
owner under both job locks, or rejoin the existing queue after that owner settles.
An already captured terminal delivery cannot absorb another subscriber. Cancelled
or obsolete running readers retain their real custody while newer intent waits in
the existing material-serialized queue.

Occupied Copy now retains its own strict `pad_copy_overwrite` journal, immutable
A/B musical seals and one fixed fresh C through preparation, own Native ACK,
actual Config receipt and terminal settlement. Independent A/B/C original and
stem readers survive queued old-target cleanup and uncertain append retries.
Restart distinguishes exact original-input replay from a recognized written C:
replay needs the original A/B; recognized C obtains fresh target-only Native
acceptance while surviving-source changes remain intact. Saved selected stems
must acquire their own current registration and demanded component/preload ACKs
through the existing verification path before settlement. Complete history,
lineage or custody conflicts retain visible fences. See
[startup ownership](architecture.md#persistence-and-restore) for the controller
sequence and [Portable validation](portable-windows.md) for kind routing.

This is an internal software foundation. Public Re-Arrange gestures, complete
selection/Hold matrices and whole R2 acceptance remain open, as do musical,
resource, device and Human gates. It does not expose the pending
Key/transposition performance milestone.

## Scheduled pitch across an occupied Copy — 2026-10-17

The device-free public stereo regression now composes a real occupied Copy with
an old target's original pitch attack. Source A first adopts piano +12 and offset
-5 through real captures, workers and scheduler effects. Its complete musical
snapshot carries both nonneutral components into fresh stopped C when A replaces
occupied B. The old B action cannot attack or retune C, whether B already had a
prepared candidate or Pitch and Copy are processed in either queue order.
Admitted B actions retain their frozen execution frame; Copy-first processing
rejects stale B before scheduler admission. The terminal reason is `stale_content`.

For both Key Lock modes, an independently constructed A reference checks exact
PCM, source progression, Native/FIFO chronology and DSP history through Copy's own
acknowledgement, mirror and finish, across the old effect frame and beyond a full
preparation lead. C has a lifetime distinct from A and B, retains the copied
tuple, and remains stopped until its fresh offset-step retrigger. That original
C capture adopts its own prepared Native and source binding and contributes
actual additional PCM while A continues independently.

This profile uses FullMix and complete-source replay. The fixture checks actual
productive source/resident bindings; bounded-window preparation still requires
its tap certificate, whereas complete-source replay has no such certificate.
Host readiness waits freeze virtual time and do not establish a device deadline.
Productive selected-ALL overwrite, the full selection/Hold matrix, DSP quality,
resource, device, musical and Human acceptance remain open. Production audio and
scheduler code are unchanged by this regression block.

### Selected-component extension

Six additional profiles exercise the same original pitch/occupied-Copy orders
with both Key Lock modes, A and C using mask 15 and B its own mask 5. They require
current own registrations, demanded component acknowledgements, slot-consistent
leases, distinct A/B/C assignment IDs, exact retained A reader/component owners,
and actual source-specific wet Native output. A still matches an independently
constructed reference sample for sample through Copy and the old B effect frame.

C's stopped component receipt is explicitly waiting without a voice or attack.
The same-content pitch Admit preserves its cancellation revision, so that receipt
may remain current; it is never treated as an audible acknowledgement while
stopped. After C's fresh attack, the test requests current mode confirmation and
requires both its effective ACK and actual current-source wet output. A generic
wet flag or a mode ACK alone does not satisfy this test. Native readiness is
established before the Copy comparison, not asserted at the first attack sample.

Mask 15 sums to the canonical FullMix source in this fixture. Selection/demand,
real component owners, distinct mask 5 and actual Native-source matching therefore
supplement PCM equality; this is not an independent acoustic masking oracle.
Four further profiles compose stopped selected C with a genuinely prepared future
pitch retrigger, then call public pad Stop or targeted launch cancellation in both
Key Lock modes. These operations advance C's cancellation revision without changing
its content lifetime or source assignment. The original action retains its capture
and frozen T/E and terminates `launch_cancelled`; it neither starts C nor changes
its copied pitch. C's old component receipt loses current/effective authority,
while the selection registration itself stays current. A keeps its revision,
readers, component owners and living Native and matches independent reference PCM,
FIFO and source/DSP history across the original effect frame and another lead.

A fresh component receipt is accepted/current but waiting while C remains stopped.
Fresh C input then adopts its own selected-source Native and current wet mode; the
old action and receipt stay rejected. Targeted cancellation here concerns a stopped
pad with a pending-only attack; it does not replace the ordered Stop required for
an already living voice. Broader selection/Hold/cancellation matrices, complete
R2, DSP quality, resource, device, musical and Human acceptance remain open.

## Durable intent and the snapshot

`ProjectState.pad_stem_intent` is the single musical stem-selection authority for
all 216 slots. Its versioned immutable value contains the enabled mask, remembered
custom mask and custom/instrumental/all display mode. Mask zero is valid. FullMix,
Delete Stems and unavailable components retain this intent; a completed new source
assignment or actual unload resets it. Legacy projects receive independent
defaults. Invalid rows recover visibly without replacing the original malformed
config; valid neighboring rows survive, and Copy rejects a known config fence
before obtaining a Native reservation.

An ordinary launch can invalidate the output observation of a previously accepted
stem receipt. Polling withdraws that observation and its matching historical retry
interest without changing the saved ALL/mask choice or releasing current audio
owners. A fresh explicit selection obtains its own receipt; independent preload
interest and actual Native failures retain their existing control paths.

`CopySnapshot` seals canonical validated JSON rather than retaining mutable model
children. It includes source/material birth metadata, analysis and selected pair,
timing/manual/TAP, loop/grid, Key Lock inheritance, key correction/base/extra/
retrigger, Gain/EQ and stem intent. Every projection revalidates and creates fresh
models. The target gets a fresh content UUID and historical `copied_from` lineage.
That lineage never grants permission to look up an originating slot.

Current source/timing/selection permits, runtime lifetimes, voices, DSP history,
cursors, hold effects, meters, pressed state and worker handles are excluded.
Persisted analysis evidence remains historical; only a current Native owner and
its own acknowledgement can make it eligible for playback or verified export.
Cold assignment and Copy allocate checked runtime lifetimes outside realtime.
An explicit replacement, empty slot or different complete source digest gets a
fresh lifetime; same-material window/timing maintenance retains its content's
lifetime. Current bindings and queued cold messages check that lifetime as well
as existing source/epoch authority. Rejected pending work restores only its exact
predecessor, and a genuine unload clears the active lifetime. Runtime numbers
are scoped by the receiving Native ownership instance and never persisted as
permission.

## One complete acceptance and durable publication

`PadArrangementController` owns one Copy at a time:

1. Capture the musical seal and fresh identity, acquire the existing config writer
   and retirement reservations, then reserve the actual empty Native target.
2. Prepare independent source custody, suitable shared PCM/views, selected pair and
   any accepted timing rebind on the existing bounded cold-worker lane.
3. Reserve assignment images before enqueueing one complete Native Copy payload.
   Preparation and enqueueing do not advance current target epochs.
4. The callback validates capacity and current guards, claims once, installs the
   complete stopped source/timing/stem/mask/Gain/EQ/key image and acknowledges last.
5. Reconcile that exact target receipt, merge the snapshot into the latest project,
   atomically save config and publish the new assignment custody.
6. Project target-only UI state and its own accepted stem-selection ticket, then
   release the Native hold and config writer.

The callback moves prepared handles and fixed scalar state. File/JSON/Python work,
verification, preparation and destruction of residual owners stay off realtime.
A distinct Copy parameter epoch prevents older Gain/EQ and legacy timing messages
from rewriting the installed image when the two message queues drain in different
orders. Target gestures and conflicting inheritance/tempo-anchor changes reject
before intent or queue mutation while settlement remains pending.

Preclaim failure can cancel without modifying occupancy or current epochs. A
claim without an observed complete ACK keeps owners and fences; an ordinary source
ACK cannot certify Copy. After actual adoption, config or callback failure retries
under the same retained transaction rather than guessing rollback. Unrelated newer
project edits survive the candidate merge and retry.

Source and selected-pair assignments use independent verified subscriptions.
Shared PCM is charged once per resident account, while every actual owner view
and physical reader remains represented in existing capacity accounting. Removing
the originating slot therefore cannot retire a surviving copy's material. The
last assignment, voice, queued work, retained ticket and physical reader still
participate in final reclamation.

Warm restore also reuses equal finite ranges. Complete original/cache validation
and held FileID checks run before consulting bounded weak range locators on the
existing cache. Equal ranges retain the same live PCM and immutable source
descriptor; each restored pad still builds and admits its own view and obtains
its own Native acceptance. The existing opening/preparation gates serialize equal
cache misses. Locator locks are released before resident accounting, and dead
views are pruned without retaining PCM or file ownership. Different ranges and
different verified interpretations cannot reuse this backing.
Concurrent recovery can briefly hold an unused warm staging marker. That exact
identity-bound directory then transfers to the existing reserved cleanup queue;
the verified cache remains usable and unrelated generations remain protected.

## Closing and verification boundaries

Close requests stop Copy admission and try genuine preclaim cancellation while
keeping controller custody until the actual preparation worker returns. A claimed
Copy completes its normal own-ACK/config/callback path before dependent controllers
or audio owners shut down. The UI keeps close pending with compact, viewport-sized
progress while the existing deduplicated console reports technical settlement
errors. The close poll continues even without a rendered GUI frame. The shell gates `app_shall_exit` in
`after_swap`, which the
[pinned HelloImGui loop](https://github.com/pthom/hello_imgui/blob/902dd7d8229b0f396a7f5d4e0a61486825b69c4a/src/hello_imgui/internal/backend_impls/abstract_runner.cpp#L1162)
runs before the next exit check. `before_exit` performs only permitted teardown.

The focused control tests distinguish scripted adapters from actual Native proof.
Native public tests use generated owned WAVs, real canonical import, real worker
preparation and the actual callback without opening a device. They exercise
targets 36 and 215, shared backing and independent custody, source continuity,
negative admission/queue/cancel/stale/claimed cases and productive config/assets
publication. Separate saved-QM and new-process tests cover their corresponding
evidence and lifetime boundaries. These software cases do not certify musical
quality, device behavior, listening, the full 216-pad resource matrix or later
Re-Arrange/HoldRelease integration.

The R1 software gate passes with these actual Native proofs, the impacted control
regressions, strict affected OpenSpec validation and an independent source review.
A private optimized Release passes API/Portable/import checks without installation;
537 focused model/control/UI tests also pass against that private Native module.
The earlier broad Native failures and their individual repairs remain recorded in
the local test log; this is not a claim of a fresh whole-workspace test run.

## Hold/Pair controllers and durable completion — 2026-10-18

The HC-18 integration now joins the real Python `WaveformController` and
`PairArrangementController` with genuine Native Hold/Pair tickets, the boxed
callback mixer, actual Config replacement, journal records, assignment leases,
both controller mirrors and Native finish. This complements the earlier Native
composition and the separate Python tests with a scripted Native facade.

`control_hold_tests/controller_native_tests.rs` executes eight isolated child
profiles: Move or Swap, release before or after Pair publication, and release
before or after the pause acknowledgement. In each case B has its own previously
accepted pause. The original physical press/ticket remains the controller owner
until its actual terminal Native observation. The Pair acknowledgement records
whether that owned effect preceded or followed the layout claim. No direct
Native finish or preparation substitute bypasses the Python controller.

The probe writes two matching 128-frame stereo PCM16 sources at 48 kHz. Their
Native source publications, starts and independently paused B are genuine; the
small App/project envelope is synthetic. Public `_prepare_pad_pair` uses its
normal no-device stereo format. Original mono fixtures retain their mono path.
Preparation runs synchronously on the test control thread, outside the callback;
the next ordinary controller poll collects and publishes its original result.
This is no cold Loader, neural inference or device proof.

After the actual callback drain, the test verifies the exact Native cohort,
content lifetime, generation, sample backing, source position, Native/FIFO and
DSP history. Real controller settlement must use its own acknowledgement, write
the same-revision Config bytes/digest, reopen a settled journal, publish live
Native assignment leases at the exact destination-to-source file paths, retire
displaced assignment owners, clear observer fences and release the original
reservation. Persisted content, numeric pitch
intent, loop/EQ/gain, selected content, waveform zoom and independent B pause
follow their surviving contents. A subsequent 96-frame stereo render must equal
an independently continuing unchanged-layout fixture exactly and remain finite
and non-silent. This neutral FullMix profile does not replace existing nonneutral
pitch, accepted-timing, selected-ALL or productive wet-history profiles.

Fixture negatives remain recorded: the initial generic PyO3 extraction failed
compilation; incomplete stereo setup first rejected the ordinary start as
`prepared_mismatch`, then rejected its mono source as `source_changed`. A Path
object inserted into `sys.path` prevented imports. Finally, the deliberately
seeded Native manual timing and the initial legacy Python label disagreed, so
the strict journal rejected the committed image. The probe now asserts matching
initial timing intent and uses actual stereo sources and a string import path.
No production guards or assertions were weakened and no production defect is
inferred from these invalid fixture states.

The independent final review found that the first custody oracle could accept
an unchanged pre-Swap assignment table. An executed negative with the actual
live before table reproduced that gap. The final oracle requires both exact
absolute destination paths and released displaced leases; only unaffected B in
Move may reuse its original tuple. Every profile also checks that its unchanged
before table cannot certify the after-layout custody.

The focused eight-profile Debug gate passes. All 42 affected Native Pair tests
pass in 53.46 seconds before the custody-oracle refinement; the unchanged other
41 cases are reused. The final eight-profile Release repetition, including the
before-table negative, passes in 11.94 seconds. Changed Python lint, formatting
and typing, Rust formatting and strict affected OpenSpec validation pass. Clippy
passes with 95 existing lib-test warnings and no references to changed Rust files.
Production code, API and packaging are unchanged; the private Release/API/import
checks from delivery `96c07db` are reused without a new installation.

The broader Python Pair/mirror/observer/assets/Hold batch remains **failed**:
223 passed and two failed in 674.61 seconds. Both unchanged cancellation tests
failed to observe their backend-entry event within five seconds. Their isolated
repetition passes both in 39.63 seconds with unchanged assertions and deadlines.
The failure cause remains unknown; pre-backend identity/file verification and
scheduling are possible delay locations, not established causes. This is no
fresh whole-suite pass, and no timeout, verification or custody gate was relaxed.

This closes the named controller-to-Native/durable seam only. The subsequent
HC-06/07/19/26 public lifecycle integration is described below. Whole R2 and public
Re-Arrange gestures remain gated. Full resource, realtime measurement, acoustic,
musical, device and Human acceptance remain open.

## Public Cold/Hold removal, replacement and Close — 2026-10-18

`audio_stream/held_lifecycle_tests` (source file `held_lifecycle_tests.rs`) joins
actual Python Loader, waveform/playback controllers, inherited App feedback and
shutdown methods with a complete production callback image, genuine Cold workers
and source receipts. The small App envelope injects the device-free engine and
wires real components without calling `AppController.__init__` or `AudioEngine.run`.
Four isolated child processes generate their own 4096-frame stereo PCM16 WAV at
48 kHz and use only their owned files. Initial and replacement source digests must
equal the SHA-256 of the actual original file. No synthetic publication, manual
loader-event consumption or substitute mixer load establishes readiness.

The unload/refill profile obtains a real caused pause and its controller ACK,
fills the actual producer with Pings and calls public Loader unload. Queue-full
rejection must preserve the Project, Session, Config bytes/revision, source
identity/digest/current binding, original physical Hold/ticket and live assignment
leases; the callback voice image also remains equal. The independent review found
that these Python observations alone did not cover Native request counters. The
final Native snapshot also requires unchanged physical/logical requests, prepared
epoch, authority, cached PCM/digest, exact Cold assignment ID/retirement/path,
Cold lease generation and loading state, acquired in production lock order.
After draining capacity,
actual unload removes the old callback source. Public loading of the same input
creates fresh content, Native generation and lifetime with equal verified bytes.
The successor starts and is independently paused through public playback control.

The direct replacement profile instead leaves the original controller pause ACK
unobserved while Native owns the caused pause. A real incoming same-file load must
adopt its own new lifetime/generation/content before the successor starts running.
Both profiles then deliver the old physical release and original captured feedback
values. An unchanged clone of A's genuine START plus separately withheld actual A
telemetry provide nonempty old status and telemetry evidence; they are replayed
without reconstruction or retagging. Native lane/generation/pause/source position,
PCM backing and Native processor address, plus the entire Session image around
feedback replay, remain unchanged for the running or independently paused C.

The two Close profiles retain the actual original Hold before its pause ACK or
after observing its owned pause. Actual App preparation must remain pending until
the original Native caused effect releases and its owner becomes terminal. Actual
App shutdown flushes Config, enqueues the real Stop and releases assignment leases.
The test drains that Stop and verifies inactive voices, then closes the complete
callback owner through the production retirement worker. A trailing destruction
witness reports only after every callback field was destroyed on that worker.
Engine shutdown alone is never counted as callback destruction. A fresh engine
then restores the actual saved Project through public Loader/Cold ACK, preserving
durable content/path/bytes while acquiring fresh Native owner/lifetime and no old
Hold/pause state. Its ordinary public start must produce finite nonzero output.
This does not assert complete process-wide final-reader or resource acceptance.

The first driver attempt rendered before ordinary captured-start preparation was
polled and failed the nonzero-output assertion. It now polls the existing residency
controller and waits for genuine callback adoption, preserving that assertion.
The next baseline passed removal/replacement but failed actual App shutdown at
`stop_all`'s stream-only producer lookup. `stop_all` now uses the existing
`control_producer()` accessor with the same lock, capacity/error handling and
`enqueue_stop_with_producer(None)` kernel. Production still obtains its actual
stream producer; this change enables the headless integration seam and establishes
no production-stream failure.

The initial four lifecycle profiles pass Debug in 4.85 seconds; the stronger
request oracle passes Debug in 4.80 seconds and its final lock-order version passes
Release in 4.84 seconds. An adjacent 13-test feedback
run initially had 12 passes and one failure; its unchanged isolated case also
failed. The GLOBAL-start route now uses the ordinary atomic pitch executor, while
that test's interleaving hook remained only on the older batch executor. A
test-only hook now runs after the actual ordinary-global commit and before its
captured feedback is published. The test helper also requires every requested
interleaving to execute. Original receipt and stale-resolution assertions remain
strict; all 13 feedback tests pass in 5.06 seconds. Callback-close two and launch
publication nine regressions pass. No production feedback/DSP path was changed
by the hook repair.

Private Release compilation, eight existing API checks and real App/UI/backend
imports pass without installation. Clippy passes with the same 95 existing
lib-test warnings; no new test-module warning or changed-line warning is added.
Python lint, formatting, typing and both affected strict OpenSpec validations pass.

The first impacted Python run had 211 passes, two skips and one failure: an old
Copy test refused any ALL STEMS request solely for missing registration. The
existing residency specification instead permits fresh current-source capture
and bounded verification while residency waits for its own accepted registration.
The unchanged isolated case reproduced that stale expectation. Two replacement
tests cover the actual boundaries: a copied disk pair may queue verification
without registration/residency/publication/availability, and missing loaded source
authority refuses with unchanged Project and zero Native calls. The first new
positive fixture forgot to replace its deliberately absent preparation method
with a Mock, failing its no-call assertion; that setup is now explicit. Existing
preparation tests also retain pending-registration and own-ACK-before-residency
coverage. No production stem guard changes.
The final eight-file Python integration against the private Release passes 263
tests in 17.09 seconds with two existing skips. These controller tests use their
scripted Native facades; actual Cold/callback effects are established separately
by the four Native profiles above. The single independent final review is clear
after the request-oracle correction.

This closes only the named HC-06/07/19/26 software joins. The earlier Python225
batch's two unresolved backend-entry failures remain recorded above. At this
boundary full R2 still required its separate nonauthor closure and explicit
HC-13 runtime-tick/retarget and HC-15 rejected-Pair-with-owned-Hold assessment.
Public R3 gestures and all resource, realtime, acoustic, musical, device and Human
gates remain open.

## Owned Hold, rejected Pair and actual editor retarget — 2026-10-18

The child module `held_lifecycle_tests/retarget_tests.rs` joins the two previously
missing HC-13/15 executions. Two sequential isolated engines each publicly load
the same generated 16,384-frame stereo PCM16 file through actual Cold workers,
start A and B, independently pause B and cause A's original Native Hold through
the actual waveform UI action with real headless ImGui button state. They run
the same callback/render chronology with independently initialized DSP histories.
No callback owner is cloned to provide the reference.

The tested engine fills its actual producer with Pings. A public Pair swap then
prepares but fails publication before claim. Its original ticket remains captured
with no own ACK. The controller's actual Pair-only shutdown preparation cancels
that same operation, records durable cancellation and releases reservations and
the writer. The original Hold stays owned throughout. Before images require
unchanged Project, Session, Config bytes/revision/dirty state, source digests and
bindings, exact live Python assignment owners, Native physical/logical requests,
prepared epochs, authority/cache/Cold assignment/loading state, transport and
launch revisions, slot/content/binding epochs and both voice histories.

Reservation intentionally fences fresh selection admission: Native projects the
same selection revision with no admissible slot while reserved and the exact
original selection after cancellation. Reserve and release each publish one
seqlock marker, so the test requires exact layout-publication increments of two
and four while retaining each actual slot/content/binding epoch. These markers
are not a content move. The initial stronger-than-contract full SlotSnapshot
equality failed on that expected marker change; source inspection of the actual
reserve/release path established the corrected exact oracle.

The actual UI selects B and opens its editor. A genuine worker-produced A view
must have been ready first; retargeting cancels A's Native view request to idle,
advances the view revision and replaces the cached A projection with B data.
This is the actual view lifetime, not an invented persistent waveform PCM lease.
The existing global runtime tick observes real button-up outside the controls and
performs real App polling without rendering the original toolbar or using a
release/poll facade. Only the original A effect resumes, exactly once; independently
paused B and both original processor/FIFO/position/source/filter histories remain
unchanged. Repeated ticks enqueue no further release. The next 96 stereo frames
must be finite, nonzero and exactly equal to the independent reference, which
releases the same kind of owned Hold through the ordinary direct controller route.

The first driver failed because the ImGui binding cannot restore a nonexistent
context with `set_current_context(None)`. Context restoration now follows the
existing isolated UI fixtures and restores only a prior real context. Together
with the marker-oracle correction this changes test setup only; production UI,
controller, Native DSP and callback code remain unchanged. The combined Debug
profile passes in 4.32 seconds. Final Release passes both test entries and all
five isolated lifecycle/join profiles in 7.54 seconds after a 1m57 build. The
three adjacent pressure/cancellation, reservation/matching-release and GUI/Native
selection regressions pass. The fresh locked, stripped private Release wheel
builds in 32.35 seconds; its eight bootstrap API checks and actual App/UI/BS
imports pass without installation. Impacted Python integration against that
private Native passes 108 tests in 9.04 seconds. Lint, formatting, typing and
strict OpenSpec checks pass. The independent review determines the separate
scoped R2 software closure; this entry does not declare full R2 acceptance.

The single independent final review is clear and accepts separate R2 SOFTWARE
closure using the prior 19 assigned HC cases plus this actual HC-13/15 join.
The software prerequisite for beginning canonical R3 implementation is therefore
satisfied. R3 gestures receive no advance acceptance; full R2 and all final
resource, realtime, acoustic, musical, device and Human gates remain open.

This synthetic FullMix software join supplies no recorded-music, acoustic,
resource, realtime, device or Human acceptance. The earlier Python225 batch's two
unexplained backend-entry failures remain open; unchanged prior wet/timing/refresh,
Hold/Pair, custody and lifecycle results retain their original profiles.

## Re-Arrange pad interface and original source admission

The transient Re-Arrange control now owns a complete mouse press before ordinary
pad trigger/stop handling. It starts OFF per UI context, changes no Project mode
or MIDI switch, and exposes left Move/Swap and right stopped Copy/Copy-overwrite
through the existing complete Pair/Copy controllers. Warning styling, short help
and an explicit source/target/operation preview accompany actual target hover,
including hover blocked only by the source button's active ImGui ID. Consumed
release remains owned through the entire rendered frame, preventing bank or
normal-pad actions even if mode changes mid-drag. Outside release, Esc, focus,
settings and Close cancel without dispatch. Bank mutations remain the separate
R4/R5 scope; active pad mode permits no bank navigation or drag-release action.
Crossing START/STOP also consumes the original pad gesture before its held-button
transport handlers, including after mode-off cancellation. Fresh independent
START/STOP presses and middle-button mute retain their existing behavior.

The gesture retains the exact Native source binding and durable source identity
from press. A Python-only currentness check cannot close a race with asynchronous
Cold adoption. Copy therefore forwards the original binding to its existing
Native request-lock validation. Pair retains that same object across journal
retries and validates original owner, source slot, current projection, generation
and lifetime under the existing request lock before any reservation. Same PCM,
digest or even generation cannot authorize a new lifetime. A bound source
rejection follows actual durable cancellation rather than fresh recapture or
unending stale retries. Legacy admissions retain their existing no-binding route
through the same Native capture kernel. The matching stub and Portable preflight
require the optional source-binding keyword without constructing an engine.

Two actual global-mute regressions were reproduced before repair: volume edits
could publish audible gain during middle-hold mute, and outside/focus/Close
release depended on the local START/STOP handler even when Learn intercepted it.
The existing volume controller now retains the latest clamped persisted intent
while effective output stays muted. The existing global runtime tick restores
that latest value once on middle-up/focus/Close before App polling, including
hidden and Learn controls; still-held focused mute remains global. There is no
new audio or scheduler path.

The final optimized Native run passes three entries and eleven isolated profiles:
all four rendered arrangement operations, same-path source replacement during
drag, held A plus global mute and a continuing B, and the five existing lifecycle/
retarget profiles. Actual Cold loading, original gesture binding, Pair/Copy workers,
callback ACK, Config and finish remain connected throughout. The original source
lane/PCM/processor/history survives Move/Swap; Copy creates its own stopped target.
Middle-up restores the continuing peer at the latest persisted volume while A
stays held; right-up releases exactly that original moved Hold. Five original-source
guard tests and four adjacent complete Pair regressions also pass in Release.

The fresh private Release passes eight bootstrap API checks and actual App/UI/BS
imports without installation. Initial broad Python integration passes 2012 tests;
its only failure was an older metadata renderer's incomplete UI double. After
updating that double, affected UI/input integration passes 861 tests. The real
START/STOP crossing regression first fails all four cases, then passes; after
the final handler extraction, all 104 impacted bottom-bar/gesture/runtime/render
tests pass. The independent middle-mute path remains covered. The initial mute
regression's nine failures and all test-driver corrections are retained in the
workspace test log. Headless render doubles remain separate from the actual
Native effects proved above. These software results do not close musical,
resource, realtime, device, listening, Human or full parent acceptance gates.
The single independent final review is clear and accepts the separate R3 software
closure for these pad gestures, original source admission and global mute/release.

## Bank36 Native and durable foundations

The current R4 foundation captures one complete source/target bank image, including
Empty cells, and publishes one aggregate Native operation. All participant
reservations share the existing ContentPlacement claim. A fresh occupied target
is stopped with its own mutable state; an Empty source clears its corresponding
target. The source and outside voices keep their actual history and Holds. Old
target voices, Holds, samples and stem readers retire together off realtime. The
aggregate reserves 133 retirement entries, rather than multiplying single-Copy
capacity by 36. Reconciliation and finish require the same opaque parent ACK.

Preclaim cancellation retains the parent reservations while an admitted child
kernel still owns preparation. The existing ColdJobs lane now offers terminal
owned delivery for Bank; ordinary callers keep their established early-return
policy. Cancelled results are destroyed before terminal return. Poisoned queued
owners retire outside the queue lock, including after cancellation was already
latched. A claimed or accepted operation cannot report rollback; an uncooperative
running kernel may still delay terminal cancellation.

BankSnapshot seals exactly 36 ordered musical rows. The pad_bank/v1 journal keeps
the original whole-project/Config binding, immutable source/before/fixed-after
seals, all phase ordinals and actual same-revision Config text/digest. Historical
recognition accepts the complete before or complete after bank and rejects a
mixture of changed cells. Later compatible source/outside intent survives fixed-
after projection. Occupied seals supply inputs to existing independent asset
custody; Empty witnesses require scalar fences. Historical data supplies no saved
Native authority. Native Portable recognition now validates this dedicated bank
kind, including all history ordinals and exact current schema, while retaining
historical Pair/Copy compatibility. Its complete 43-test Debug suite passes. Two
independent-review findings have real regressions and are fixed: raw availability
is checked before normalization, and fixtures provision their own owned roots.
All eight final Bank entries also pass in Release; the remaining 35 Portable
neighbors retain their compatible earlier Release evidence.

Caller-original Native BankParticipants now retain the actual ordered occupied
and Empty incarnations before confirmation and admission. The same original
guards are checked under the existing complete reservation marker and retained
through effect claim. Eight new callback tests cover real replacement/ABA, stale
authority, Pending, unrelated publication and actual component availability.

Normal controller and aggregate mirror foundations have bounded software proof:
111 mirror/shared-neighbor tests and 19 controller tests pass, with one independent
review clear. Those Python tests script the Native boundary. Three additional
real callback/controller cases now pass in Debug and Release: mixed productive Native/FIFO/
Hold continuity against an independently initialized full 16,384-frame PCM
reference, full36 Copy followed by Clear with one actual parent ACK and Config
receipt each, and same-PCM/generation replacement after original capture. They
reuse the existing Copy fixture and productive controllers, journals, mirrors
and asset owners. Initial global Key Lock and FullMix commands use the existing
headless queue; this does not prove their stream-bound public setter admission.
Bank startup routing and fresh aggregate recovery now have bounded implementation
and software proof: eleven actual callback cases pass in Debug and Release,
including genuine Cold and selected-stem restoration, Empty rows, same-owner
retry after delayed Unload, queue pressure and independent subscriber retirement.
The combined public Copy/Bank suite passes 52 Debug entries. The strict startup
owner retains fixed images, all journal ordinals, source and target custody, and
the existing Config writer; original-before recovery transfers a durable child
into the same normal Bank controller. Its Python tests substitute the Native
boundary. App now binds this owner before ordinary restore, composes mutual
admission, gesture and observer fences, exact queued feedback and shutdown, and
refreshes Material cleanup only after actual Copy/Pair/Bank inactivity. Explicit
retry uses that same ownership guard even when disk history is already terminal.
The affected 298 Python cases pass; independent review is clear after fixing that
retry guard. The existing real callback/production-feedback neighbor also passes
in Release; its partial App facade now represents the added idle Bank owner.
Fresh-process Bank recovery now also passes a genuine three-process Debug
profile: an actual pending producer exits, its normal durable recovery child
receives a real Bank ACK, the actual Config replacement loses its return, and
a third process obtains fresh all36 acceptance before its own current receipt
and ancestor compaction. It preserves the fixed after identities and reuses
the original/PCM/stem files. FullMix acceptance ignores its dormant component
mask while preserving its durable intent; ALL STEMS still requires an exact
effective mask. The separate positive and negative Native regressions pass.

The receiver then retains a real Instrumental reader across normal Clear of
every sharing bank. After that reader releases, all five original/source-PCM/
stem-WAV/stem-PCM/descriptor paths actually disappear through existing retirement
workers, with Config and independent sentinels preserved. The test exposed a
completed startup coordinator retaining its verification owner; Copy, Pair and
Bank now drop that reference only after all terminal cleanup succeeds. Three
additional scripted-Native controller tests prove failed cleanup retains the
same owner and successful retry allows collection while App/startup remain alive.

Two further actual Bank/SharedStemJob Debug profiles cover a surviving original
follower's own generated component ACK, and final-interest Clear with a physical
backend reader retained until its stale result returns. Newly copied equal
material inherits no job token. Only the declared nonneural output backend and
headless shape routing are substituted. These first two profiles complete the backend
after settlement; the third active-writer ordering is covered below.
Combined controller/neighbor Debug checks pass seven entries; the three startup
Python files pass 112 cases, with the three new custody tests passing separately.

R4 now has a separate accepted nonauthor SOFTWARE closure for beginning Bank UI.
Final Release checks include all36 Bank cases and three actual SharedStemJob cases.
The third completes its same prepared pair while Bank owns the common writer:
the existing queue retains its original reservation/readers, then its own disk
selection and complete ALL acknowledgement settle after Bank. Ordinary-to-live
selection preserves only an outstanding original attempt and latest desired state
through the existing withdraw-or-settle protocol; settled historical intent is
not replayed. The final targeted Python suites pass213 and120 cases respectively.
Capacity133 is also tested at132 with the same retained queue head and no partial
36-slot effects, and actual Clear/refill ignores three releases from an old Hold.
The existing genuine three-process recovery and all-five physical final-reader
retirement proofs remain compatible. Bank UI is now implemented through the existing
six selectors and common confirmation owner: left-click another bank copies without
navigation, occupied targets require exact OVERWRITE, and right-click any bank requires
exact CLEAR BANK. Cancel/Escape/close retain no transaction; acceptance consumes the
same original request once. A consumed PAD drag also disables the underlying Bank
widget so a staggered second-button release cannot activate it later.

Actual App testing exposed automatic MIDI refresh replacing reserved participants
with unloaded runtime rows, invalidating their original Bank guards. Python now keeps
the last successful fenced rows; Native retains its complete existing rows and pitch
shadow under the same atomic runtime publication, rechecking original reservation
owners, placements and revisions. Unrelated MIDI remains publishable. An explicit
Multi Loop change retains its original all-row semantic revision and may supersede a
pending Bank; no stale captured trigger is preserved. Post-settlement Clear/copy rows
refresh normally. Unsolicited Native rejection has explicit console feedback without
fabricating an error for deliberate cancellation. Five focused Native boundary cases,
two genuine InputMapping/Bank/ACK/Config profiles and the actual rendered Bank join
pass in Release. Final affected suites pass39 Bank,119 input-mapping,9 binding and20
pitch-action cases, with1308 Python cases. These are separate profiles with their own
inputs, not whole-product or hearing acceptance. Full R4/R5/HC acceptance remains open.
P6/216/resource, neural, musical, device and Human acceptance remain separate.
Pending target preparation and
committed selection collectors currently reject admission before reservation,
leaving their original owners reachable; arbitrary pending replacement is open.

Fourteen actual Bank callback tests pass in Debug and Release; affected Pair,
placement, binding and Copy mixer regressions pass. The combined public Copy/Bank
suite passed 41 Debug entries before the eleven new recovery cases. The three new Bank cases, fresh-process Copy
restore and both PitchFirst neighbors also pass the final optimized profile.
Fresh A/C onset comparisons retain every emitted frame against an independent
stereo C ABI/PCM reference; B contributes real audio before
overwrite, and the earlier live wet-output guards remain. These are bounded
software checks, not musical or listening acceptance. The durable foundation
passes 40 focused tests. The shared projection update
passes 283 combined journal/snapshot tests with its neighbors and 43 final
snapshot/export cases.
The full-bank validation workflow exposed repeated construction of a 216-slot
project for each copied row. Bank now reuses the existing strict Copy-after
projection directly, retaining all validation boundaries. The measured full-bank
test falls from 69.73 to 8.19 seconds, and one profiled record revalidation from
2.481 to 0.214 seconds. These are diagnostic measurements, not UI-latency
acceptance. Full gate and negative-result accounting remains in the workspace
test log. R4 and R5 retain their existing ordering and acceptance gates.

## Pair settlement across a later Multi Loop change

An explicit Multi Loop publication still advances the current content cells'
runtime policy and revokes incompatible captured input actions. Before a Pair's
Native claim, the existing original guards may therefore reject and cancel it.
After its own Native acknowledgement, the Pair must finish that acknowledged
mutation under the latest policy. Reserved old MIDI control rows retain their
exact captured image until the control mirror; each mirrored successor receives
the current runtime epoch of its acknowledged destination content cell.
Source, lifetime, placement, own-ACK and original-control comparisons remain
required. No stale row or changed source becomes admissible through this rule.

The reported post-ACK row mismatch is reproduced with a real InputRuntime.
Thirty-five focused Native cases pass, including four policy toggles and unequal
participant epochs. Two actual App profiles also pass Swap and Move through
Config/journal settlement, current selection and shutdown preparation. These
bounded checks establish neither device acceptance nor the separately reported
copied-loop reader failure. That exact failure remains unreproduced in four TAP
finite-source/launch/controller profiles and one genuine acknowledged Automatic/QM
profile. The additional Copy/Seek/Move/Swap and Voice/Bank compositions described
below extend this bounded coverage; they still do not reproduce that Human error.

## Copied finite readers through Seek and later placement

The continuation adds actual Copy, intro/tail Seek, Move into an empty cell,
Swap with an independent copy and subsequent loop/complete-source preparation.
Collection runs before and after the original Pair Native ACK and control mirror;
each moved content retains its assignment and lifetime while neighboring copies
retain independent loops. Both Key Lock modes are covered.

Separate cold-adoption cases establish a genuinely different active Voice and
current Bank window through the existing material subscriber lane. They cover
both the same immutable source and a different source, unload the original PAD,
release replacement-admission handles and collect before seeking the retained
copied voice. Loop preparation then resolves the current Bank's own assignment.
The fixture retains its original complete material seed, distinct from these
finite Voice and Bank PCM allocations. These are bounded ownership regressions,
not audio, device or Human acceptance and not evidence of a product fix.
