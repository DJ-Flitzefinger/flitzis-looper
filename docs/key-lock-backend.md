# Key Lock Backend

This document records the current Rubber Band based Key Lock implementation.

Key Lock is one bounded part of the Rust audio/DSP foundation. It does not imply
plugin hosting, a separate FX graph, or realtime stem generation.

## Runtime Modules

The pending [pad-owned PCM program](pad-owned-pcm-program.md) gives finite DSP
supply its own P4b proof/implementation slice. Current saved KEYLOCK still uses a
labelled full-track fallback for unproved continuation. Dry loop tap parity does
not prove native feed/lookahead/FIFO/history/seam coverage or causal availability.
Keep the1598-frame/33.292-ms fixture, Rubber Band and existing rate/nonzero-k/unity/
latency gates; neither range caching nor directory migration removes acoustic
latency. No fallback-only intermediate state completes the requested finite supply.

The first P4b vertical supplies a stopped/current Normal loop through the retained
FullMix/four-component readers, its own WindowWork callback ACK and the real
Key Lock preparation lane at production k=0. `native_source_coverage.rs` binds the
complete source, actual resident window/revision, copied read plan/playback,
timing projection and selected stem owners. Both actual fractional taps use the
same mapping as rendering and must lie in held physical ranges. Normal-loop
closure covers every phase and canonical binary64 rate-smoothing chunk; 4096
output frames are the native adoption horizon, not a guessed source halo.
Real native preparation transfers its actual handle and input/output FIFOs, then
callback rendering continues with finite input. Pending history requires its
exact window; already adopted same-source history survives a storage-only ACK
without resetting native/FIFO/filter chronology or dropping its original pins.

`KeyLockFiniteLoop` has snapshot fence4 and the `key-lock-finite-loop` metadata
label, but the tag does not grant coverage or source/window/history authority.
Missing input fails preparation/render admission. Finite seek/intro/tail and
physical-domain controls now use the bounded context proof described below;
unproved contexts retain their guards and the whole P4b gate remains open.
Current saved-load KEYLOCK retains its
labelled complete fallback; normal-loop control preparation can obtain finite
supply. The full P4b gate and device/acoustic acceptance remain open.

A stopped transposed voice whose Native pitch scale is exactly unity still starts
immediately with transparent source PCM. If a later global speed/master-period
edit needs its first source-owned wet history, the previous effective global tuple
and audio remain in effect until one common output boundary. The existing source
workers prepare the old trajectories to that boundary. Complete participant,
source, timing, selection, voice and retirement checks precede adoption of every
required Native/FIFO owner and the joint rate/transport effect. Existing productive
owners retain their history across canonical rate smoothing; a frozen pending
candidate still requires its exact original target. Stop, Pause, supersession or
unavailable preparation refuses the old rate effect as a whole. The Pitch sidebar
distinguishes requested speed from confirmed audio speed/period and pending or
rejected state. This bounded handoff does not close continuous-from-birth, acoustic,
resource, performer-deadline or Human acceptance.

The public nonstarting SET history proof additionally covers eight stereo profiles:
44.1/96 kHz, both Key Lock modes and total -23/+24. Actual source rates reach 0.5
and 2 before a further off-grid transition remains active at capture and adoption.
Distinct left/right source and held-stem signals feed one independent two-channel
ChannelsTogether Native reference. Every sample of 4097 adopted output frames
matches exactly in Debug and optimized Release, with original T/E, action, source,
voice, phase and FIFO ownership retained. Each channel's incorrect FullMix-only
history differs. Queried Native history bounds range from 13312 to 59904 frames;
block-lattice alignment can extend replay. The reference uses the declared
2048-frame Short window at 44.1 kHz and 4096 at 96 kHz. This
extends bounded A-reset evidence; continuous-from-birth, acoustic and live timing
acceptance still require their separate gates.

The separate public variable-rate diagnostic fixes a 48 kHz stereo Normal loop,
off-grid rate knots .73/1.41/.62/1.29 and four destination tuples: total -23/+24
under both Lock modes. Two distinct analytic tones use the predeclared interval
E+24000 through E+48000. All eight channel measurements meet the unchanged
15-cent limit; the largest absolute error is below 5.835 cents. Every one of the 48000
post-E stereo frames also matches the independent A-reset reference exactly.
The continuous-O destination reference uses its own original first-block pitch;
its PCM differences remain separate observations.

Thirty-six separate isolated fixtures place an impulse, an 8-ms source-domain
tone burst or percussion at source frame 138163 plus 0/17/511. These are source
offsets, not output-phase offsets. Public output retains its actual pre-E part,
while raw A and O retain their complete original responses. The shared fixed
metrics preserve pooled stereo energy, centered 0.5-ms quantiles and each full
response's own denominator and first original peak. Without an accepted timing
compensation, all 36 A and O responses fail q10 and q50 against the dry source
clock: q10 offsets span 3557..7069 frames and q50 3812..8665 frames. Their original
peaks, fixed-E prefix-energy and final-20-ms tail checks pass in this bounded
capture. All public cases fail at least q50. Two public original peaks precede E,
and a hypothetical cut at E would remove up to 15.94% of a public response's
energy. That pre-E audio was actually audible under the old tuple; it was not
discarded by the implementation. No delay, gain, crop, target or threshold is
fitted to these measurements. This diagnostic closes no acoustic, continuous-O,
music, device, performer-deadline or Human acceptance gate. The optimized run
also emits six Native insufficient-output messages, requesting 512 frames and
reporting 511. Successful assertions do not resolve or attribute those messages
between the actual owner and the two independent raw reference owners.

A later opt-in test marks actual-owner capture, independent bound/constructor
work and every completing Raw A/O block separately. The six formerly affected
fixtures passed without Native output errors, as did the subsequent regular
40-profile run using the same rebuilt binary. These non-reproductions neither
attribute nor resolve the original six messages. The DUT workers remain alive
during reference replay, so markers alone cannot assign a future interleaved
message without separate thread or process isolation evidence. The actual loaded DLL version
has not been established by this evidence; the upstream 4.0.0 implementation
described in earlier audits pads a missing output sample rather than reporting
the deficit through the void C API.

The Windows opt-in diagnostic now runs each of those same six Key Lock ON
profiles in three fresh, serial child processes: the real public DUT, raw A
(including its independent history-bound queries), and raw O. The parent creates
no Native state. The DUT retains its original production execution and exports
the independent scalar input as exact f32 bits; each reference consumes those
same bits with its original rate chronology and first-block pitch. Original
PCM, FIFO, source/receipt and uncropped acoustic criteria remain in force. Raw
output, PID, exit status and warning counts are saved separately for every child;
cross-role comparisons run only after all children are terminal. This requires an
existing verified absolute `LOOPER_NATIVE_DIAGNOSTIC_ROOT` with fresh profile
directories. The actual imported function is resolved through its verified x64
PE import thunk when needed, then checked against the loaded DLL export, path
and file hash. All children must use identical executable, Python and RubberBand
files. The observed RubberBand DLL has SHA256
`4a8d323cc8c40d16413bbffe459811f93ebb5f7249b9e830bf4d11c62e4feb6d`;
its PE file and product version fields are unavailable. Header/API constants do
not establish its library version. A future warning can identify a process role;
that alone cannot identify a particular DUT worker or instance. Missing warnings
still do not attribute or repair the historical messages.

The optimized isolated run completed all 18 children without an insufficient-output
or Resampler message. Every post-E A-reset sample matched exactly and final FIFO
states agreed. All six profiles still failed both q10 and q50 for public, A and O
responses; the fixed failure mask was 3 in each case. The original warnings remain
unresolved: fresh-process isolation changes preceding process history and removes
cross-role concurrency. The focused regular Debug tests also retained all 40
original profiles and their recorded acoustic negatives. Production code is unchanged.

One bounded mixed-origin retrigger proof uses a live +7 Normal voice and one
selected +7 endpoint. GUI +7, routed MIDI -7 and GUI +7 are captured before ACK
for the same future quantized T, then published in order 1/3/2. The actual
scheduled renderer preserves old PCM before T and executes all three original
actions in capture order. Only the first consumes the cached Native; subsequent
actions use their own preparation. A complementary fixture observes each event
through the original scheduler and the same production executor at frozen T,
checking distinct Native/request/source/tuple ownership. Both fixtures compare
all 48000 final output samples with an independent ordinary-start worker.
This proves software effects for Key Lock ON/SYNC OFF with this source and
timing; it does not claim three separately audible attacks at one sample, a
Loader cold-source path, device timing or broader acoustic acceptance.

The bounded resident-selection context additionally admits FullMix->Stems,
Stems->FullMix and enabled-mask changes for active current-source NormalLoop
voices with the identical already ACKed resident committed StemSet. An inactive
verified pair publication can retain that set while FullMix is selected; after
playback starts, the existing scalar native mode/mask commands use this real
Some/Some path. Their prospective read plan and productive feed validate both
target and active `transition.from` against the actual four component backings,
matching source/window/timing and canonical fractional/rate trajectory. Empty
masks do not stand in for absent owners. The existing ramp advances by 128 source
frames, including fractional progress and its existing interruption policy.

The wet native handle, input/output FIFOs and chronological filter continue
through the entire ramp. Automatic source-specific preparation remains deferred
while the ramp is active, then actual settled-selection catch-up and guarded adoption
use the existing worker and exact Source/Window/Stem/History permits. Storage-only
ACKs do not restart chronology or release still-used pins. The retiring
`SetStemPairFullMix` path releases components only after the outgoing ramp; history
and job readers retire separately through the existing registry/recycle paths.
The ordinary StemController FullMix path uses that retiring command. Its active
initial None->Some, warm return, retention policy and effective-mode ACK remain
separate [P5a work](pcm-cache-residency.md#active-same-set-residency-p5a-2026-10-11).
No resident-transition result alone closes those workflows or the remaining P4b
matrix.

The bounded lifecycle/source-custody implementation keeps finite NormalLoop
physical geometry fixed at
production k=0/non-unity. Pause/resume before and after preparation capture,
settled or smoothing rates and admitted storage-only ACKs must preserve actual
position, adopted native handle/FIFOs and chronological filter. Pending work is
still exact-window/projection/selection/checkpoint-bound and stale work cannot
change current audio. Only already adopted history may retain the proved
storage-only effective-source permit while its original real readers remain.
Wet continuation must extend past the first resumed callback through subsequent
real worker requests/preparation/adoption, not just retained state fields.

Same-source accepted-period refresh/clear uses the existing narrow chronological
projection alias across virtual P+/-H seams, with unchanged physical region.
Source/marker/position cuts cannot borrow it; old active or paused voices reject
replacement-bank refresh. Bank B cannot relabel voice A's source, timing,
FrozenStemView/transition or history. A new A preparation requires fresh actual
Voice/Source/Timing/Selection/History capture and matching own permits. A retained
PCM pin cannot authorize native work or revive a pre-replacement bank candidate.
Future B obtains real WindowWork admission/ACK; that ACK alone grants no native
permission. Fresh public capture and independent Source/Timing/Stem/History
permits still govern preparation and exact adoption. Explicit retrigger is a
DSP/history cut into acknowledged B, reserved before effects; failed admission
keeps old audio, parameters and owners.

The launch executor uses `trigger_loop_context_available` for this cut. Its
old-source exception requires the requested physical region to equal the current
bank's already installed region and the real bank Source/Timing/KEYLOCK coverage
to pass `can_play_sample`. Fresh trigger binding and retirement reservation still
precede effects. Timing and batch refresh retain the unchanged-physical/current-source
alias guard. Explicit physical marker controls instead prove the copied new
trajectory and both selection sides before effects. Identical finite Normal
marker reapplication remains a no-op; timing changes use their separate accepted
paths without rebasing the source cursor.

Lifecycle proof uses independent complete PCM and algebraic raw-native/filter
references, with irregular callbacks crossing pause/resume, rate ramps, refresh/
clear and ACK/adoption. It observes real taps/feed/lookahead, native identity,
FIFO occupancy, cursor and continued nontrivial wet output. Distinct voice,
source, stem, job, history, reader-pin and outgoing-selection final uses and
actual old/new held peaks use existing retirement/recycle paths. No hidden
complete backing, fifth instrumental component or new reader/queue is admitted.
Accepted Some/Some evidence may be reused by unchanged source impact only. The
bounded lifecycle subtask is independently accepted. The finite seek/domain
implementation uses existing CaptureResidentSeek/WindowWork/control ACK and
Native history lanes at production k=0. Intro admits prefix plus loop and Tail
loop plus actual source end; these ranges cover all later smoothing/lookahead
taps without a guessed horizon halo. Retained source A uses its original lease
and fresh retained-voice Native permits independently of replacement bank B.
Explicit seeks, including equal-phase requests, advance voice generation and cut
Native/FIFO/filter history while retaining the actual rate target and smoothing.
Physical edits compare both next interpolation addresses and their weight;
real jumps reserve a clean warmed handle before resetting usable old audio.
Storage-only full-to-finite return can retain adopted chronological state and
original pins, while pending work stays exact-window/checkpoint bound.
Complete-backed/non-KEY Seek and explicit ALL exceptions remain supported.
The [finite seek/domain report](finite-seek-domain-validation.md) records bounded
software evidence and failures. Full P4b, initial finite KEYLOCK-load/P5b, P5a,
remaining resource/music/history/causality, nonzero KEY/pitch and Human acoustic
acceptance remain separate.

The active backend is implemented behind:

```text
rust/crates/looper/src/audio_engine/stretch_processor.rs
rust/crates/looper/src/audio_engine/rubberband_backend.rs
rust/crates/looper/src/audio_engine/key_lock_preparation.rs
rust/crates/looper/src/audio_engine/prepared_native_history.rs
rust/crates/looper/src/audio_engine/native_history_permit.rs
rust/crates/looper/src/audio_engine/native_source_coverage.rs
rust/crates/looper/src/audio_engine/source_playback.rs
rust/crates/looper/src/audio_engine/source_reader.rs
```

`RtMixer` owns tempo-ratio target selection, per-pad Key Lock state and per-voice
source/processor calls. `source_reader.rs` centralizes
effective loop regions, explicit seek progression, full-mix/stem compatibility,
integer source addressing and source-selection crossfades. It borrows accepted
buffers and can be reused by background preparation without importing the mixer
or native processor. `source_playback.rs` owns scalar fractional source epochs
and active-output-frame ratio smoothing. The mixer uses that cursor with the
reader's linear interpolation taps and advances stem transition ramps by
fractional source distance.
`VoiceSlot` owns the per-voice source playback state, `StretchProcessor` and
explicit seek mode. The mixer retains one preparation worker for its 32 voice lanes.

Productive `StretchProcessor` fills its own fixed feed from the actual borrowed
source through the shared reader and canonical cursor. Its consumed native
history and pending FIFOs bind that source's address/shape/loaded rate and full
effective accepted revision with exact period/signed-origin bits. The expected
next fractional position, including seek mode, guards continuity before samples
enter native or pending adapter state. See [native timing ownership](native-constant-timing.md).

G3b2f1 supplies continuous productive history ownership. G3b2f2 extends the same
worker with actual source-specific native/FIFO preparation and timed transactional
adoption described below. That productive gate precedes G3b2g accepted persistence/
fresh loader adoption; its completion requires genuine ownership/output/failure
proof. Neutral reserves and the test-only preparation fixture stay separate.
Later B5 audible crop/delay/transition compensation is a separate gate.

## Live mode transactions and feedback

An own nonzero-pitch mode request at settled source rate 1 can retain the current
productive Native owner and all its FIFOs: ON and OFF then require the identical
pitch scale. This applies only to current complete FullMix sources without a
selection transition or source-lane work. The original request and E remain
unchanged. Every participating voice must pass fresh source/window/timing permits,
Native/generation/epoch identity, exact projected source phase and source-control
continuity together in the existing claim. A stale captured request is refused
without resetting productive history. Other changes retain prospective preparation.
The originating Native request stays immutable; a separate effective association
selects ON h/r(n) or OFF h for later actual rate changes. It cannot authorize pending
candidates and is cleared by adapter reset. This removes an unnecessary history
replacement; it does not establish general acoustic or performer acceptance.

Public SourceACK/start tests cover total -7/+7 in both mode directions: all 24000
frames from E equal independent uninterrupted old- and destination-mode owners,
and all 48000 frames of a later real 1.25 rate change match destination history,
including smoothing. Origin metadata stays immutable. Separate actual controls
cover Pause/Resume cancellation, supersession and two concurrently waiting pads
with different E frames. One pad's supersession preserves the other's checkpoint.

Global and pad KEYLOCK actions use the same Residency transaction. A pure mode
change prepares the current physical range without adding a loop edit or seek.
Genuine pending geometry/seek edits remain coupled. Active finite admission is
limited to the proved production k=0 NormalLoop feed, including both sides of an
existing selected-set ramp and each retained old voice's own source/timing/domain.
It preserves SourcePlayback, voice generation, loop region and chronological EQ.
OFF retires mode-specific Native/FIFO/candidate state through existing worker
ownership; ON can use a warmed unique handle on the actual current feed.

`pad_key_lock_status` reads a bounded callback-owned, source-fenced mode snapshot.
Its request identity and window revision identify actual application separately
from enqueue and WindowWork publication. Stopped/paused processing can be armed;
active nonneutral processing must have usable Native wet output before an ON
transaction settles. Until the first usable Native block, the existing dry feed
continues through the same EQ history; the handover uses that Native block
boundary independently of callback partitions. A worker, reserve or processing
failure before wet output preserves dry audio and reports effective OFF/error.
Admission checks both the applied rate and authoritative rate target, including
a SPEED or BPM command preceding ON in the same callback batch.
Stopped/paused arming retains that first-live-wet guard for start/resume. An
explicit owned-ON start or retrigger configures its fresh source domain before
the first coverage check and re-arms readiness for that voice; a previous voice's
wet receipt cannot certify the new voice. Waiting remains visible on the buttons
even after the armed controller transaction has settled. After actual wet
confirmation, an error reports the actual ON baseline even if UI polling missed
the ready snapshot. Terminal feedback reconciles mode without undoing already
adopted geometry through project fields. Later
source-specific candidate adoption still requires the exact output frame,
SourcePlayback, read plan and native permits; the mode snapshot does not certify
that adoption. Reloaded source generations cannot inherit old feedback.

Global actions update the default and visit loaded inheriting pads across banks,
including targets whose resolved value already equals that default. Explicit pad
ON/OFF survives later global actions. `ProjectState.resolve_pad_key_lock` resolves
the nullable override for all control/admission paths; Rust still receives a bool.
Native feedback never rewrites that durable origin after a rejection. Individual
failures remain visible while other targets settle. The main global button colors
the default; the sidebar PAD button is neutral for inheritance, green for explicit
ON and red for explicit OFF. PAD left-click sets ON and right-click sets OFF.
Adjacent GLOBAL clears only the override; it colors the default only while inherited.
Confirmed audio, preparation, unconfirmed, errors and mixed outcomes are shown
separately. Existing global keyboard/MIDI/Learn routes share this controller;
capturing Learn does not execute a toggle.

These boundaries do not open finite seek/intro/tail, nonzero KEY, new Stem-set
adoption, whole P4b/P5a, resource/music or Human hearing/visual gates.

Stream setup creates the unique mixer owner on the heap before capturing it in
the CPAL callback. Rendering only borrows that preallocated owner. The shared
constructor has a hardware-free fixed 2-MiB stack regression; it does not certify
actual device startup or hearing.

## Playback Semantics

- Per-pad Key Lock off: playback is varispeed, so tempo and pitch move
  together.
- Per-pad Key Lock on: source-frame tempo progression remains active, and the
  varispeed block is processed through a per-voice Rubber Band LiveShifter with
  pitch scale derived from `1.0 / tempo_ratio`.
- The global Key Lock control updates inheriting loaded pads only. Successful new
  track assignment clears its predecessor's override and inherits the current
  default; a failed load preserves the predecessor's intent and usable audio.
  Unloaded native slots remain disabled, with no durable override.
- BPM Lock off: the active tempo ratio is the global speed multiplier.
- BPM Lock on with valid master/source timing: the active tempo ratio is
  `source_period / master_output_period`. Acknowledged accepted source timing
  takes precedence over legacy pad timing. Supported clipping does not redefine
  the requested master period.
- Pads without valid BPM metadata use the global speed multiplier.
- Full-mix and prepared-stem playback share the same source addressing and Key
  Lock path.

## Source Timing And Resampling

Scheduled render segments carry absolute output-frame positions from
`audio_stream.rs` into `RtMixer::render_rt_at_output_frame(...)`.

Every playback mode uses `SourcePlayback` to derive fractional source progress
from a scalar source epoch plus active output-frame count times the actual native
binary64 tempo ratio. Start and render use the same native period/rate resolver;
SourcePlayback target/ramp and fractional epochs preserve binary64. Reads linearly
interpolate two integer
neighbors through the shared half-open loop/seek policy. The lookahead tap wraps
at loop end; explicit intro seeks play into the loop, and tail seeks play to
track end before wrapping into the loop. Both channels and prepared-stem
selections use the same addresses and fractional source transition progress.
For compatible effective accepted timing, virtual source phase wraps at
`P=loaded_rate*accepted_quarter_period*logical_beats` while physical integer
markers remain exact. The final admitted PCM knot interpolates to loop start at
P, safely inside admitted PCM for both P<H and P>H. Rate remains the existing
source-period/master-period ratio. Legacy/manual/tap and incompatible loops use H.
Copied live/worker read plans include exact P bits, so matching current phase
cannot authorize prepared adoption with a different future period. Rate rebases
retain virtual seam phase; the single feed still supplies dry and native output.

Rate changes rebase from the current fractional position instead of independently
rounding source consumption per segment. Pause freezes source and smoothing
progress; resume continues them. In-range loop edits retain fractional carry,
while out-of-range edits retain the existing loop-start clamp. Explicit seeks
and retriggers start a new source epoch. Integer telemetry floors the next
source cursor. Ordinary wrapping does not redefine the Rust master output
timeline, Rubber Band state, editor source grid or prepared-stem alignment.

The same source feed supplies dry varispeed output and the Rubber Band adapter.
The named inverse-rate pitch conversion uses LiveShifter's native `c_double` ABI
without narrowing to binary32. Existing near-unity bypass and pitch-update
thresholds remain; sub-threshold pitch changes are not claimed to be applied.
`StretchProcessor` no longer interpolates render-segment endpoints. This makes
source/native input independent of fixed, irregular and one-frame partitions
when source state and accepted control events are equivalent.

The separate `source_grid.rs` foundation derives source beat/bar and loop-cycle
phase from the editor's signed origin. Transport retains complete beat position
across master-BPM changes and can bootstrap once from the selected BPMLOCK
reference. These operations preserve voice read positions and Rubber Band
ownership. The master-to-source mapping helper is internal and tested; normal
starts still read the effective loop beginning. Neither that helper nor the
one-time bootstrap compensates audible Rubber Band/device delay.
Bootstrap queries a copy of the canonical source cursor configured against the
current effective loop. It maps the integer source frame plus its fractional
remainder to beats, matching the position rendering will read after loop/seek
normalization without advancing the live cursor. A changed loop cannot anchor
the master to a source position that will be clamped before the next read.
These changes do not reconstruct Rubber Band state.

## Rubber Band Processing

### Preparation Ownership

Stream setup constructs 96 unique native handles: one current handle, one
neutral reserve and one source-specific preparation reserve for each of 32 voices.
The current/neutral pair is warmed with silent blocks before callback rendering.
The third handle's exact pitch/reset and first actual source shifts run on the
worker for the captured request; it is not neutral warmed as prepared history.
Fixed block buffers, channel pointer arrays and bounded FIFOs are also allocated
before rendering. Preparation failures,
including failure to start the worker, reject stream setup.

The original two-handle neutral pool has a measurable startup/memory cost.
Historical standalone Windows measurements for 32 stereo voices observed:

| Output rate | Pool setup | Working-set increase | Private-committed increase |
| --- | --- | --- | --- |
| 48 kHz | 151.347 ms | 137.477 MiB | 146.668 MiB |
| 96 kHz | 270.090 ms | 205.965 MiB | 230.719 MiB |

These are single-run process deltas on the measured system, not portable memory
limits, live callback costs or measurements of the extended 96-handle pool.
Current [96-handle setup/resource measurements](pcm-cache-measurements.md)
are recorded separately for the production pool. Fixed reserves bound ownership
exchange; source preparation throughput remains a separate profiling scope.

The pinned Rubber Band 4.0.0 source audit found that native `reset()` and a
pitch change before the first `shift()` call `measureResamplerDelay()`, which
creates two temporary `std::vector<float>` buffers. The probe's calling-thread
Rust allocator counter does not observe these C++ allocations. Construction,
native reset, cold pitch setup, and silent warming therefore run only during
setup or on the preparation worker. The callback applies pitch changes to an
already warmed uniquely owned
handle. See the [pinned native implementation](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3LiveShifter.cpp).

Start/retrigger, stop, seek, and leaving wet processing clear only the adapter's
own bounded storage and mark used native state dirty. On the next wet render,
the voice exchanges that state for its warmed reserve through two bounded SPSC
queues. The worker resets and warms the returned handle. The exchange reserves
return capacity first and never destroys native state, waits, or prepares DSP
inside rendering. Teardown releases the worker after voice rendering stops.

When no reserve is ready or the recycle lane is full, wet rendering returns
silence for that segment and retries later; the source timeline continues.
Dry varispeed and the approximately neutral ratio remain immediate. Pause/resume
retain the current native state. Stem mode/mask transitions retain native
history and use the existing source crossfade instead of resetting Rubber Band.

Same-source accepted adoption/clear and rate changes retain chronological
native/FIFO history when the canonical next fractional position is continuous;
the complete effective accepted projection refreshes on productive feed.
Pending/rejected timing leaves it unchanged. A source mismatch or discontinuity
invalidates bounded adapter storage and marks used state dirty before foreign
feed can enter it. A source-neutral warmed handle becomes source history only
through actual productive consumption; this adds no source-specific pre-roll.

Voice source and timing remain paired when a bank sample is replaced. The old
active voice retains its pinned PCM and previous effective timing, separately
from current pad-bank acceptance. An admitted retrigger adopts the actual current
bank source, retires its old pin off realtime and invalidates old adapter history.
Successful same-source timing refresh updates only matching voices. Native
allocation/reset/loading and large-owner destruction remain outside the callback.
Explicit active seeks use the pinned voice's source extent after bank replacement;
even a same-position seek clears bounded adapter/FIFO and fixed per-pad filter
history, with native reset/warming still owned by the worker.

### Productive Source-Specific Preparation

GUI and MIDI pitch choices retain one original `CapturedPitchAction`: content
lifetime, source/timing authority, numeric tuple, input time and dueframe T.
SET and SET_RETRIGGER share the existing scheduler and fixed action custody.
Preparation selects a separate execution frame E without calculating another
grid target. An already usable live SET keeps its Native/FIFO history and applies
as a scalar effect; it does not send an unnecessary request to the worker.

Disabled mapping and Learn discard unpublished MIDI pitch receipts by cancelling
each exact Native owner. A separate 64-record control journal rewires cancelled
predecessors, so later cancellation cannot restore an already discarded choice.
It retains scalar source fences and lightweight status observations, with no
Native, PCM or action-work owner chain. Accepted requested-model projections
anchor later composition; stale source/timing records cannot overwrite them.
Journal reconciliation runs under the existing control mutex outside rendering.

Live nonzero KEY LOCK changes prepare each voice's prospective numeric tuple
through the existing source lanes while its previous effective mode continues.
All participants must pass their exact source, window/ACK, generation, epoch and
endpoint checks before one content claim exchanges Native/FIFO owners at E.
For a finite source, actual storage can be claimed first; the original mode
ticket remains adopting until its own complete effect. That storage observation
and mode success have separate custody, so mode refusal neither releases a
dependent start nor loses the actual installed window. Retirement pressure
retains the original owner. Unavailable trajectories remain visible request
refusals with old effective audio retained; paused endpoints, variable-history
and full B5 acceptance remain open.

GLOBAL START captures an opaque native group before waiting for an existing
resident edit. Its original participants, timing ACK owners, requested loop
regions and input time remain fixed. One existing callback target message freezes
T for the complete group; publication after readiness uses that same T and one
common execution frame E through the existing atomic ordinary group and scheduler.
Polling never recaptures the source or reconstructs loop/pitch intent.

Each pending resident dependency retains its actual native ticket/publication and
uses only that request's genuine adopted binding and launch permit. Move/Swap
resolve the original content key. A later numerically matching request cannot
replace the dependency. Group cancellation cancels its attacks and preserves the
independently requested edit. A pending edit without a native ticket is explicitly
unavailable before GLOBAL capture; this rejection does not claim preservation of
an accepted gesture across queue pressure.

Equal ordinary starts register separate Native memberships against one actual
physical readiness request. Each retains its own Work, sequence, T and callback
effect. Per-member cancellation cannot revoke a sibling. An independently
requested loop/mode edit is joined through the exact ticket retained by that
gesture; the original request ID and WindowACK remain authoritative, and cancelling
the attack preserves the edit. Membership lookup precedes proposal supersession;
the weak cache and bounded control-side registry retain no adapter candidate.

One selected Normal-loop start has a separate source-specific Native reserve.
The callback captures its actual bank source, loop, timing/window ACK, rate and
applied pitch/Lock tuple after control and parameter drains. One lower-priority
mailbox on the existing preparation worker owns one mutable Native/FIFO bundle;
background work consumes none of the 64 accepted-action cells. An ordinary
capture allocates its scalar owner on the control side and constructs Native on
the worker only when it misses this reserve.

A hit consumes the bundle once and replaces its cache request with the original
attack's fresh request, permit, epoch and sequence. An on-time hit keeps E=T;
late admission keeps original T and uses the current callback frame for E.
Each further attack retains a distinct receipt even when the reserve is empty.
Selection changes can retire the cache without revoking an already accepted
attack. Source, incarnation, real window, loop, rate or tuple changes prevent
reuse. A same-physical-window ACK can reuse preparation only behind that
attack's own exact ACK and current authority. The integrated Ordinary42 check
includes all ten reserve cases and complete initial PCM; this is a bounded
selected-endpoint result, with full deadline, trajectory and acoustic gates open.

`prepared_native_history::NativeAdapterState` centralizes the actual native
handle and fixed input/output FIFOs used by both productive `StretchProcessor`
and its preparation worker. The worker's bounded request/result/recycle lanes
retain an actual processed owner; a source tag or neutral warmed state does not
substitute. A request pins actual PCM and optional admitted stems, copies
canonical `SourcePlayback` and `SourceReadPlan`, and carries a `NativeHistoryPermit`.
The permit has two distinct authorities. `Current` checks tracked loaded-request
generation, source address/shape/rate, shared preparation epoch, authority/runtime
revisions and CURRENT acknowledged full accepted projection, including exact
period and signed origin bits. `RetainedVoice` requires a source admission captured
while that exact source was CURRENT, then a fresh capture of the live voice's
lifetime/generation, own preparation epoch, source/rate, frozen timing and actual
selection/history. Bank replacement invalidates pending Current work before
freezing the voice; it never converts that work into RetainedVoice permission.
Both authorities retain exact resident-window, playback/checkpoint and selection
checks for new preparation; a source PCM pin supplies neither authority.
Automatic without acknowledgement cannot capture. Manual/Tap/Legacy remain
nonaccepted; equal-valued timing edits still retire pending work. A shared local
atomic voice epoch is checked by the worker before/after catch-up; adoption checks
the exact outstanding request ID as well as that epoch.
Current/prepared `NativeAdapterState` and pending-request retention use boxes
allocated at setup. Ready/recycle/pending-return lanes move preallocated box
owners, keeping large retained payloads out of the 32-VoiceSlot stack aggregate.
Request rings also allocate fixed storage at setup. The callback only moves/swaps
these owners; catch-up feed/output scratch allocation remains on the worker.

The worker applies exact starting inverse-rate pitch before reset and processes
4096 active frames through the same fixed adapter and source reader, including
copied canonical rate smoothing. It retains the resulting handle and full FIFO
continuation with request pins. Active stem-selection transitions defer automatic
requests until complete, while effective wet playback continues through both proved finite
selections; the settled request must still complete real preparation and adoption.
This bounded horizon is not
a native-delay estimate, pre-target crop or accepted musical onset policy.

The deadline is the captured absolute request output frame plus 4096.
`StretchProcessor::chunk_until_prepared_adoption` splits productive rendering at
that exact frame. The callback checks current permit/effective full timing,
actual source/stem identities, read plan/loop/seek and full playback checkpoint
plus local request/invalidation epoch. It reserves bounded recycling capacity
before swapping the native/FIFO state, keeping the logical source cursor intact.
Pending, failed, stale, unready, late or recycle-saturated work keeps the old
effective audio/history. Rejected/old prepared owners and pinned PCM/stems return
for worker recycling/destruction; no heavy catch-up, cold native setup or final
owner destruction runs on the callback. A reset can retire source/stem request
pins through a separate bounded worker lane while retaining the invalidated dirty
native/FIFO owner; the existing dirty-state fence still prevents foreign feed.
Stop/reset/wet deactivation publishes local cancellation. Stale in-flight owners
are destroyed on the worker, with completed-request atomics settling pending state.
Paused/inactive voices poll only bounded ready/recycle retirement every callback,
so a result published after cancellation does not require later source rendering
to release its pins. Stream teardown retires remaining tail owners off realtime.

Productive ownership, genuine nonzero shifted output, independent continuation
and failure-path tests establish the numerical G3b2f2 gate. This is
distinct from the non-live proof below and supplies no acoustic delay/crop,
click-safe mode transition, device-deadline or sustained audible SYNC acceptance.
The prepared adapter matrix compares independent algebraic-source/raw-native
20,017-frame suffixes across steady/smoothed mono/stereo, rates and loop/seek/stem
cases. Actual mixer/worker tests separately prove the transferred handle/FIFOs,
12,853-frame productive raw-native suffix, current-permit/runtime rejection and
off-thread pin retirement. See [productive preparation evidence](native-constant-timing.md#productive-prepared-native-continuation)
for scope; these comparisons do not close the acoustic gates below.

### Adapter And Measured Delay

The tested SHORT + CHANNELS_TOGETHER backend uses 512-frame blocks. Wet
activation seeds the output FIFO with `block_size - 1` silent frames, so the
adapter adds a fixed 511-frame lead (10.646 ms at 48 kHz), independent of callback
partitioning. This removes the previous growing offset from underflow silence;
the lead is currently uncompensated. Missing shifted output still uses bounded
silence, with no refill spin.

Native nominal delay, measured transient onset/peak, adapter lead, and device
buffering are separate quantities. The initial optimized offline baseline used
cold ratio-specific native state and isolated impulses at output-domain frame
8192:

| Output rate | Tempo ratio | API delay frames | Impulse peak delay frames |
| --- | --- | --- | --- |
| 48 kHz | 0.5 | 2909 | 2909 |
| 48 kHz | 2.0 | 3678 | 3260 |
| 96 kHz | 2.0 | 7774 | 6844 |

These baseline values are not correction constants for music. In the final
warmed-pool probe, the 48 kHz ratio-2 adapter impulse peak was 3771 frames after
its reference: a 3260-frame native peak plus the fixed 511-frame adapter lead.
Fixed 64/128/256/512-frame callbacks and the irregular
`[64,96,257,512,31,1]` pattern produced the same peak for this fixture. The
nominal native API delay was still 3678 frames. The independent fixed-lead FIFO
model reported no underflow across 2048 calls per tested pattern.

Final optimized 48 kHz ratio-2 measurements, using 24 repetitions, observed:

| Operation | Median us | p95 us | Maximum us |
| --- | --- | --- | --- |
| Native preparation for reuse | 1250.25 | 1578.50 | 1742.60 |
| Adapter first activation and processing | 215.90 | 531.30 | 534.50 |
| Adapter Rust-only reset | 0.90 | 1.30 | 2.60 |

The baseline adapter reset median was 105.35 us and first activation median was
800.40 us. Moving preparation changes where work happens; it does not remove
the native preparation cost. Calling-thread Rust allocation counts were zero
for measured first activation, reset, reactivation, and warm processing; worker
and C/C++ allocations are outside that counter.

A handle warmed at neutral pitch retains different startup history when its
ratio changes. The API delay getter remains nominal; it does not prove the
output transient's location for that history or a changing ratio. Startup and
settled responses need separate measurement. The local baseline and final
prepared results live in workspace `scratch/`, including
`slice3-key-lock-latency-findings.md` and
`slice3-key-lock-latency-{baseline,prepared}.csv`; generated CSVs and logs are
not repository artifacts. Reproduce the probe using
[the development guide](development.md#offline-key-lock-measurement).

The adapter partition test supplies identical already-varispeed samples. The
fractional source foundation also compares immutable nonconstant sources,
loop/intro/tail boundaries and prepared-stem sums through the common source
path at 44.1/48/96 kHz. Rate changes and pause/resume use active-frame progress,
so their source feed does not change with render partition sizes. This proves
the source-to-adapter prerequisite independently of the adapter FIFO property;
it does not prove source priming, transient compensation or audible hardware
alignment. The live mixer and non-live preparation proof now call the same
`SourceReadPlan::fill_fractional_buffers` helper, also used by G3b2f2 productive
worker preparation. That native ownership/adoption contract is described above;
audible compensation remains a separate later B5 gate.

This preparation and adapter safety stage (slice 3a) does not perform track
pre-roll, delay discarding, or a separate DSP feed-ahead cursor. Source playheads,
persisted markers, the shared clock, and launch scheduling keep their existing
meaning. G3b2f2 productive source-prepared ownership/adoption extends this earlier
safety stage without choosing audible phase compensation. Short wet/bypass transitions, including
ratio 1.0 and global/per-pad toggles, remain later B5 acoustic work. Current mode
changes can still switch between delayed wet output
and immediate dry output without that transition compensation.

Output-clock snapshots estimate device buffering from CPAL callback timestamps.
They do not include native/adapter signal delay or unknown latency after the
device buffer. Offline impulse measurements and Rust allocation telemetry do
not establish hardware onset precision or live callback deadlines. Captured-input
nearest-grid diagnostics remain separate from current launch execution.

### Non-live Exact-source Proof

Native tests compile `key_lock_source_preparation.rs` and its independent
reference tests. This fixture borrows already accepted immutable source/stem
buffers; production builds and the live mixer do not include it. Both copied
cursors use an explicit constant ratio. Live smoothing and asynchronous control
changes are outside this proof. The caller's fractional clock and pending rate
target remain unchanged.

Exact initial inverse pitch uses the shared binary64 inverse-rate conversion and
native `c_double` pitch ABI used by live processing.
It is set before native reset and the first source-content shift. Reset initializes
the previous native output hop from that pitch. The fixture advances only future
feed during preparation and retains native state plus fixed input/output FIFOs.
For block size B and experimental discard D, it feeds
`P = ceil((D + B - 1) / B) * B` frames and retains raw output `[D..P]`.
Initial occupancy Q=P-D is at least B-1. After n rendered frames, input backlog
is n mod B and output occupancy is Q minus that backlog. Logical playback advances
by n; feed advances by P+n. It inserts no second silent adapter lead.

The separate oracle resamples immutable source samples algebraically and shifts
them through a raw native handle. It compares every retained and continued stereo
sample, source cursor and FIFO count across 44.1/48/96 kHz, fractional ratios,
loop/intro/tail positions, selected stems and unequal partitions. Explicit discard
endpoints verify that D is honored independently of nominal native delay.

The release CSV reports the uncropped response translated by D, startup-peak
clipping and discarded energy, alongside the retained response. Negative residuals
and a discarded original peak must remain visible: cropped output alone can hide
the lost first transient. This is evidence for selecting a later compensation
strategy, not audible acceptance. Reproduce the proof and CSV using
[the development guide](development.md#offline-key-lock-measurement).

The optimized Windows proof passed 984 reference comparisons, including 180
impulse cases with identical metrics in 45 rate/ratio/marker groups across four
partitions. In this exact-pitch/reset fixture, nominal discard clipped the startup
peak at ratios 1.37 and 2.0 at all three rates. At 48 kHz/ratio 2, discard 3678
removed 96.499% of the marker-zero response energy. Its uncropped translated
onset/1%-onset/peak residuals were -3044/-2144/-420 frames; cropped metrics were
0/0/+96. For the settled marker at frame 8192, the corresponding residuals were
-3055/-2187/-418. These describe this native fixture, excluding the live adapter
lead and device delay. They reject nominal discard alone as a startup rule and
motivate explicit source history plus a separately justified musical timing
criterion. Local evidence is `scratch/slice3d-{source-preparation.csv,validation-summary.json}`.

### Explicit Source History And Musical Criterion

The test-only request now accepts a declared earlier source epoch and history H
in output frames. Forward progression at the exact requested ratio must reach
the logical frame, fraction and seek mode. This preserves intro/tail provenance
after entering the loop without guessing a backward path. Positive history
rejects active stem-selection ramps because their earlier state is unavailable;
zero history preserves the original fixture. The path declares hypothetical
prepared content, not an actual record of previously played controls.

Native processing starts at that history origin. Raw discard D remains an
independent experiment; effective translation is D-H. With raw transient time R
and independently resampled dry time T, both measured from the history origin,
the residual is R-D-(T-H). The logical cursor begins at H while feed begins at
the processed P. Some diagnostic requests allow P<H; live source alignment is
a separate gate. Ready FIFO/native continuation accounting remains unchanged.

The bounded sweep uses H=0/8192/16384, phases 0/17/511, impulse, 8-ms source tone
and damped percussion bursts at the existing five ratios and all three rates.
Translations are API-2B, API-B, API and API+B; each compares all four render
partitions to the same independent raw suffix. Isolated responses contain no
overlapping loop transient. Separate nonconstant source tests cover loops,
intro/tail and full-mix/subset/all-component stems with fractional origins.

Before measurement, the exploratory engineering budget is set to two ms for
both q10 attack and q50 body timing: pooled stereo squared energy, smoothed by
a centered 0.5-ms box, defines cumulative-energy q10/q50/q90. Signed envelope
centers before frame zero preserve startup-reference energy without adding
causal filter delay. Candidates must retain the original response peak, discard
at most 0.1% response energy and leave at most 0.0001% in the capture's final
20 ms. These are engineering budgets, not universal perception thresholds.
Both attack and body constrain alignment without correlating different dry/wet
carrier pitches. Uncropped onset/1%-of-original-peak onset, peak and q90 remain
visible, including negative residuals. Retained onset uses that same original
peak threshold. Cut-to-silence and ready-FIFO/native-continuation jumps expose
cut/join continuity; they do not prove click-safe mode transitions.

The exported summary intersects integer timing and exact energy/peak retention
bounds across all nine phase/signal fixtures for each fixed rate/ratio/history.
An empty interval rejects constant translation under this declared criterion,
including untested integers; it does not rule out a different content/onset
policy. Three sampled phases are diagnostic coverage, not exhaustive phase
acceptance. Reproduction commands and both CSV exports are in the development
guide. No compensation is selected by the fixture.

The optimized Windows sweep passes 6480 exact-suffix comparisons; all 1620
candidate/fixture groups have identical metrics across four partitions. None
of the four tested translations passes all nine fixtures at any fixed
rate/ratio/history. The interval check also rejects every integer translation
for all 36 nonneutral groups under the declared criterion. Only the nine
ratio-1 groups have feasible unswept intervals; live ratio-1 already bypasses
shifted processing. Twenty groups fail timing alone, and all captures meet the
tail bound. For 48 kHz/ratio 2/H16384, common timing bounds [3240,2867] are
already empty, and retention limits C to at most 2080. Nominal C3678 clips the
original impulse peak and discards 96.714% energy; q10/q50 residuals are
-907/-420 frames. Source history therefore establishes coherent provenance,
but cannot by itself make an intrinsically spread transient fit both timing
budgets. Resolve the onset/content policy before choosing audible compensation or
compensated live onset. Local measurements are `scratch/slice3e-source-history{,.summary}.csv`;
these isolated fixtures do not measure busy musical context or a device.

G3b2f1 binds actual continuous history; G3b2f2 productive native/FIFO preparation,
complete permits and timed adoption/catch-up are described above. Audible compensation
and source-aligned mode transitions remain later B5 work. No transport, marker,
launch policy or live fallback changes in
this proof gate, and no device measurement is claimed.

## Continuous Onset And Content Feasibility

A test-only gate separates native envelope deformation from a launch cut. If
`a_p = raw_qp - dry_qp`, then `a50-a10` and `a90-a50` do not depend on translation.
For integer output-frame times, the best unconstrained scalar minimax error is
`ceil((max(a)-min(a))/2)`. With a permitted integer interval `[L,U]`, the minimum is
`max(ceil((max(a)-min(a))/2), max(a)-U, L-min(a))`, provided `L <= U`.
The uncropped original peak and 0.1% energy budget impose the retention upper bound;
the reference is never recomputed after cropping. q90 remains diagnostic, outside the
existing q10/q50 acceptance criterion.

The unchanged isolated sweep contains 405 unique fixtures, 324 at nonneutral ratios.
The q10/q50 scalar lower bound exceeds the original two-ms budget in 82 of these
324 fixtures; preserving peak/energy raises that count to 234. At 48 kHz/ratio2/
H16384/marker0, the impulse displacement centers are 2771/3258/3317 frames. The
q10/q50 integer bound is 244 frames (5.083 ms), and retention requires C<=2080.
These are bounds for the measured responses, not a claim about every musical signal.

The longer-history experiment holds target-relative source content and native block
phase constant across H32768/H65536, with silent or nonzero periodic stereo history.
Varied tonal and percussive attacks probe exact ratios 0.5/1/2 at 44.1/48/96 kHz.
An attack-minus-background output is a nonlinear sensitivity diagnostic; it can
include changes to subsequent background processing. Compare it with the isolated
native attack and report actual mixture metrics too. Whole-history energy quantiles
can be dominated by background and are not an attack acceptance criterion.

One fixed offline candidate starts with canonical dry varispeed for 2 ms and then
uses a 5-ms raised-cosine transition into nominally translated continuous wet output.
Both channels share the coefficient at each absolute output frame, and the wet suffix
after the transition must remain exact. At ratios2/0.5 the dry portion changes pitch
by +12/-12 semitones; a timing result cannot certify Key Lock quality. Retain raw wet
clipping/energy evidence even if the added dry content makes a cropped envelope look
better. This is a falsification experiment, with unchanged budgets and no live policy
selected. It starts no device and changes no source grid, clock, marker, backend option
or runtime ownership. Task7.2 remains pending until an acceptable content policy is
justified; identity/adoption and synchronized Quantize must wait.

The release matrix contains 432 actual fixtures and passes 864 exact nominal-suffix
comparisons. Every nonneutral common-translation group remains infeasible. At ratio2
all 72 isolated cases fail nominal timing/retention; the bridge improves timing alone
in 46 cases but passes the complete criterion in none. Nominal cut loss reaches 99.9825%
of isolated response energy. At ratio0.5 only 10 of 72 isolated bridge cases pass the
complete criterion. Neither nonneutral background paired group has an individual full
pass. Ratio1 gives the passing neutral control in both scopes.

Isolated PCM, energy times and deformation are identical across the two histories.
Matching raw output windows from H-2048 through H+delay+capture differ for the actual
nonneutral mixture, with relative L2 up to 1.95859/1.88705 at ratios0.5/2. Paired
diagnostic q times change by up to 10339/20321 frames at ratios0.5/2, with relative
nonadditivity L2 up to 13.3664. Such long-tail context sensitivity cannot establish a
converged attack metric or be reduced to additive target energy. Duration, frequency
and phase are varied jointly; no marker0 or exhaustive musical content is covered.
The declared markers17/511 are mapped to rounded source frames (at ratio0.5 those
source coordinates correspond to output18/512). The independently read dry reference
supplies actual timing, including interpolation and the attack envelope.
These results require actual target-local continuous musical references and a pitch-preserving
policy; neither more scalar fitting nor this dry bridge is sufficient.

### Fixed Unity-Source Attack Candidate

The separate `key_lock_source_pitch_probe.rs` test module evaluates the same two-ms hold and
five-ms raised-cosine transition with a constant ratio-1 source branch. It copies the exact
logical fractional source phase and uses the existing reader; independent algebraic taps
verify its samples, including loop/intro/tail and prepared-stem addressing. Every fixture uses
its own continuous raw-native reference with matched origin, history, exact ratio and initial
pitch/reset order. H32768 and H65536 remain separate contexts, not interchangeable references.
The original wet suffix must remain bit-exact after the finite transition.

This branch preserves source carrier pitch by reading p+n, but the canonical tempo path reads
p+r*n. At 48 kHz the seven-ms endpoint is 336 output frames: ratio0.5 is 168 source frames ahead
(3.5 ms of source, seven ms of canonical output); ratio2 is 336 source frames behind (seven ms
of source, 3.5 ms of canonical output). No compensating rate ramp hides that discrepancy.
The mixed transition can still interfere, alter an attack envelope or miss a later target.
Mathematical source-rate preservation alone cannot establish perceived mixed-output pitch.

The nonzero-history matrix covers ratios0.5/1/2, 44.1/48/96 kHz, markers0/17/511 and the six
existing tone/percussion variations. Launch-local and independently source-mapped target-local
seven/40-ms windows report actual mixture energy/envelope differences and q10/q50 displacement
against matched wet and source references. Known event coordinates include source rounding.
Background can dominate a nonzero window; a launch window excluding the attack is not attack
evidence. Weighted source/wet energy and their signed cross-term expose interference. These
are diagnostics with no newly invented passing threshold. A short envelope also retains carrier
phase; a two-ms source excerpt cannot support a general spectral pitch estimate.

The earlier native q10/q50 two-ms, peak and 0.1%-energy bounds remain failed and unchanged.
Added source content cannot count as retained discarded native content. No live compensation,
identity/adoption, transition or synchronized Quantize policy follows from this offline candidate.
Reproduction is documented in the development guide; generated evidence stays in workspace scratch.

The release probe passes 648 independent unity-reader and 648 exact native-suffix comparisons
across 324 actual mixtures; all later wet suffixes remain exact. In the finite target40-ms
windows, native-relative q50 differences reach 15.927 ms at ratio0.5 and 28.027 ms at ratio2.
At ratio2 the target7-ms candidate/native energy ratio ranges from 0.227 to 271.319. These
background-containing, independently normalized window diagnostics describe substantial local
replacement/interference, not isolated attack timing or a newly defined pass/fail budget.
Seventy-two targets lie after the fixed bridge; exact target-window output there proves only
unchanged wet continuation. The unity-source bridge is not accepted as a general onset policy.

### Causal Scheduling Limit

The prior isolated raw retention bounds also define a necessary scheduling condition. With
nominal translation C and maximum translation U retaining the original peak and at least 99.9%
of raw target-response energy, preserving that content needs at least max(0,C-U) frames before
the nominal musical anchor. This is not the first infinitesimal onset and selects no correction.
For the 48 kHz/ratio2/H16384 impulse, C3678 and U2080 require 1598 frames (33.292 ms).
At 120 BPM a nearest future 1/64 boundary offers at most 15.625 ms, before device/control lead;
many future targets offer less and past targets offer none. Across the prior matrices, maximum
necessary headroom is 36.875 ms (slice3e) and 34.286 ms (slice3f). These bounds use isolated raw
responses, not background-dominated mixture energy or nonlinear subtraction.
At fractional grid-frame coordinates, integer target rounding can add less than one frame to
the continuous half-step bound (with the timing helper's small integer/tie tolerance); the
48-kHz/120-BPM example has exact integer grid frames. No such rounding resolves the deficit.

### Explicit Frame Mapping And Policy Alternatives

The test-only causal probe fixes the coordinate system before comparing policies. H is the
declared output-frame source history, n is an index in that fixture's uncropped native output,
C is a declared translation, T is the timestamp-selected musical target, and E is the earliest
controllable estimated audible frame. The hypothetical mapping is `t = T + n - H - C`.
The probe uses native API delay for C as an illustrative anchor, not an accepted acoustic or
musical compensation. Neither raw peak nor dry q50 is silently substituted for that anchor.
CPAL buffering, dispatch and preparation readiness constrain E; count each constraint once.
Unknown later hardware delay remains outside the current estimate.
Invalid or unavailable clock/readiness mapping supplies no certified E or feasibility result.
The probe assumes valid finite coordinates and enough declared history; it does not implement
production fallback or infer unavailable source history backwards.
Both raw discard H+k and logical coordinate H+S-T must lie in their bounded preparation/source
domains, and the coherent suffix must actually be ready; arithmetic alone cannot supply it.

If emission starts at S, the first retained native index is `n = H + C + S - T`; write
`k = n-H = C+S-T`. U is the largest k retaining the original isolated native peak and at least
99.9% of its original response energy. Cutting at U can still omit up to 0.1% energy; it does
not preserve every nonzero sample or define the first acoustic onset. U is not derived from a
background mixture, cropped response, supplied dry audio or attack-minus-background difference.
With `L=max(0,C-U)`:

| Alternative | First permitted emitted frame S | First retained k | Content consequence |
| --- | --- | --- | --- |
| Strict first sound at/after T | `max(E,T)` | `C+max(0,E-T)` | All mapped pre-target native output is omitted even when it was controllable. |
| Hypothetical early native content | `max(E,T-L)` | `C+max(E-T,-L)` | May emit before T; for C>U, isolated retention requires `E<=T-L`. |
| Late entry, E>T | E in both alternatives | `C+E-T` | `[T,E)` is unavailable; preserve the original chosen T and advance musical/source phase. |

For C>U, strict on-time gating fails the original retention bound even with unlimited offline
preparation. Headroom helps only the alternative that permits earlier sound. If C<=U, strict
retention additionally requires `max(0,E-T)<=U-C`; late entry is not automatically a retained
attack. At C>U, `T-L<E<T` permits some early output but cannot satisfy the original bound.
At E=T the two alternatives coincide. Postponing the target could buy preparation time, but
changes the authorized nearest-boundary contract and is not an automatic fallback.

For the illustrative 48-kHz impulse, T-E=750 frames at the best 1/64 midpoint still leaves
848 frames less headroom than the required 1598. At E=T the strict/early cut is k3678;
at E=T+240 (five ms late) it is k3918. Sufficient headroom permits k2080 in the early
alternative, while strict gating still cuts at k3678. The original failed dry q10/q50 interval
does not become feasible in any of these cases. Earlier emission cannot undo native deformation.

The causal probe uses the same canonical source reader and independently initialized continuous
native reference for each history, exact ratio, pitch/reset order and native block phase.
Emission at S advances the logical source by `r*(S-T)` from the declared target position for
constant ratio r; negative offsets use the explicitly available forward history. A production
tempo-changing path would need musical-time integration and effective-loop wrapping instead.
The prepared continuation must match the reference suffix exactly under unequal partitions.
This verifies no extra waveform deformation after the cut, not correct audible timing or pitch.

The fixed synthetic matrix covers 44.1/48/96 kHz, ratios0.5/1/2, H32768/65536, marker0 and
two existing tone/percussion attacks, with separate isolated and actual nonzero stereo mixtures.
It compares E at 40 ms and 15.625 ms before T, at T, and five ms after T. These are declared
causal cases, not a fitted translation sweep. In a fixed mapped [-40,+40)-ms mixture window W
relative to T, omission intervals are W intersected with `(-infinity,min(E,T))`,
`[min(E,T),min(S,T))`, and `[T,E)` when E>T. These represent unavailable pre-target content,
controllable pre-target gate crop and authorized late content respectively, and are disjoint.
Their sum is the total omitted window prefix; no late interval is counted twice. Finite mixture energy
describes actual missing context; it is not isolated attack retention or a perception threshold.
Suffix equality cannot certify the discontinuity between silence and its first retained sample.

The release proof passes 576 exact suffix comparisons across 36 mixtures/288 policy cases.
All 24 nonneutral fixtures still fail the original nominal complete criterion. Strict gating
retains the original isolated peak/99.9%-energy budget in none of those 24 fixtures, even with
40-ms headroom; the hypothetical earlier gate retains it in all 24 with that headroom. With
the illustrative 1/64 future interval, it retains all 12 ratio0.5 fixtures and none of the 12
ratio2 fixtures despite emitting before T. These are measured isolated cutoff predicates,
not accepted musical timing or evidence that all mixture context is preserved.

### Product Decision And Acceptance Boundary

The outstanding decision is whether T means the first permitted sound, or a musical/source
anchor that permits earlier native content. The latter needs an explicit insufficient-headroom
rule: the current nearest target and phase catch-up can preserve available continuation while
reporting unavailable content, but cannot promise complete transient retention for every press.
The former preserves silence until T while accepting a separate pre-target crop; authorized
late catch-up alone does not authorize that on-time crop. Neither alternative currently meets
the unchanged strict dry-timing/retention criterion.

Before live adoption, an accepted musical anchor must be justified on matched continuous
recorded-track references and listening evidence, with click-safe entry and device/deadline
checks. Continuous-native conformance can supply an additional no-extra-deformation check;
it cannot retroactively turn the failed two-ms/peak/0.1%-energy gate into a pass. No new threshold,
anchor, target delay, live scheduler policy or backend option is selected by this proof.

### First Original-Source Offline Stretcher Comparison — 2026-10-16

The test-only `history_probe::stretcher_probe` compares the existing 48-kHz,
source-rate2, H16384 impulse against original-source offline Stretcher processing.
The immutable stereo source contains 161,538 frames and its impulse at source
32768 maps to output16384. Duration q=.5, pitch p=1, output80,769 frames and all
anchors precede measurement. Both R2 and R3 use the default standard window,
ChannelsTogether, ThreadingNever and a maximum native process size of512.
The separate Live reference reads the independent varispeed source with p=.5;
its complete native capture has84,480 frames, including native startup and rounding.
Neither route receives a fitted offset, gain, crop, target or threshold.

The semantic origin remains0->0; only positive keyframes32768->16384 and
161538->80769 are passed to native. The first probe mistakenly passed an explicit
0->0 entry. R3 emitted14 NaN/reset warnings and initially processed at unity;
that failed setup is retained as a negative, not valid q=.5 acoustic evidence.
The published [R3 map implementation](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3Stretcher.cpp#L380-L394)
divides the first target/source anchor. Omitting the redundant native origin
preserves the complete original geometry. Constructor and first-process queries
then both report q=.5,p=1; the corrected runs emit no native warnings.

| Complete uncropped route | q10/q50 offsets from unchanged dry, frames | Energy before original H | Combined fixed criteria |
| --- | --- | --- | --- |
| Dry control | 0 / 0 | 0% | Pass |
| R2, native512 or irregular | -191 / 508 | 16.490483% | Fail |
| R3, native512 | 32 / 41 | 5.307944% | Fail |
| R3, native irregular | 62 / 72 | 3.192099% | Fail |
| Independent Live | 2771 / 3258 | 0% | Fail |

R3 meets the unchanged96-frame/two-ms timing budget in this one fixture.
Suppressing its pre-H output nevertheless discards more than the allowed0.1%
of original response energy. Strict-gated q10/q50 are40/42 for native512 and
70/73 for irregular input; those passing timing values do not repair energy
loss. R2 strict-gated q10/q50 are510/511. Original response peaks remain after H
and final20-ms energy is zero in these captures. Every wet combined raw/strict
criterion remains false. A single impulse supplies no15-cent pitch proof.

Direct R2 output is bitwise equal across the two native call partitions.
Corrected direct R3 differs at7,868 stereo samples, maximum absolute error
.4067295016720891. A512-frame process-size ceiling does not normalize calls.
The bounded offline ingress adapter stages at most512 frames per channel,
verifies every submitted sample against its original source, and sends exactly
315 native blocks of512 plus one final258-frame block with identical draining.
Both outer partitions (316 versus787 calls) therefore make316 native calls.
All80,769 stereo output frames are bitwise equal across adapter partitions and
each is exact to the direct512 reference: six complete comparisons, zero tolerance.
This establishes adapter partition independence; the direct R3 failure remains.
Published [R3 process/consume code](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3Stretcher.cpp#L658-L809)
provides a caller-boundary-sensitive mechanism, not proof of this DLL's exact build.

Each of eight offline owners completes its entire study pass before processing,
receives the original input exactly once per pass, finalizes once, drains to-1
and retires on the same offline thread. Runtime identity reuses the existing
loaded Live-import binding check: DLL SHA256
`4a8d323cc8c40d16413bbffe459811f93ebb5f7249b9e830bf4d11c62e4feb6d`;
its PE version fields are unavailable. R2/R3 engine queries and the published
v4.0.0 mechanism do not establish the loaded library's semantic version.
The previous Live C3678/U2080/1598-frame headroom counterexample also remains.
No production backend, scheduling, installation or callback changes follow from
this bounded result. Variable maps, nonzero pitch, finite live handover, continuous
history, resource/deadline/device and Human listening gates remain open.

### Rejected Causal AR2 Source-Filter Candidate — 2026-10-16

`uv run --no-sync python scripts/pitch_causal_model_probe.py` runs a bounded
NumPy-only falsification without Native, an audio device or production changes.
It reuses the original161538-frame stereo impulse at32768, and adds frozen
one-second mono900.37-Hz and331.37/997.19-Hz-plus-impulse sources. The same
source-rate2 map targets16384 and produces80769 captured frames. Each fixture
also has a rate1/pitch1 identity control; rate2 uses k=-7,0,+7.

One stereo-pooled biased Yule-Walker AR2 fit uses the whole known source offline.
The exact source residual is retimed by causal innovation scatter at ceil(m/r).
Synthesis changes pole radius to rho^r and angle to h*omega, retaining every
nonzero innovation. Causal here describes synthesis support given prepared
coefficients; whole-source fitting supplies no streaming-causal inference proof.
This is an LPC simplification motivated by the source-filter equations in
[Thornburg and Leistikow, sections2–3](https://dafx.de/paper-archive/2003/pdfs/dafx12.pdf),
and does not implement that paper's complete Kalman/filterbank algorithm.

The original impulse maps exactly, with no pre-target samples or tail and no
energy change. All three identity controls pass; maximum absolute reconstruction
error is5.533e-12 against the predeclared1e-9 numerical limit. Source frequency
calibration is within.001725 cents. The fixed Hann/4x-FFT scoring window ranks
actual strongest local peaks; it does not search near requested frequencies.

| Mixed source, rate2 | Requested frequencies, Hz | Strongest measured pair, Hz | Maximum error |
| --- | --- | --- | --- |
| k=-7 | 221.163 / 665.544 | 662.740 / 1994.380 | 1900 cents |
| k=0 | 331.370 / 997.190 | 662.740 / 1994.380 | 1200 cents |
| k=+7 | 496.494 / 1494.097 | 662.740 / 1994.380 | 500 cents |

Every mixed profile fails the unchanged15-cent limit, so this fixed candidate
is rejected. A fixed synthesis filter changes gain and phase of persistent
residual forcing; rotating its poles does not transpose that forcing. In these
profiles it remains at twice the source frequencies regardless of requested h.
This counterexample concerns untransformed residual forcing, not every causal
model or every higher-order decomposition.

Mono pitch alone is within.035 cents, but1.79–1.93% of captured output energy
remains in its final20ms and the recursive state is nonzero. No infinite-tail
bound, canonical q10/q50, complete finite-tail or overall acoustic acceptance is
claimed. Float64 analytical synthesis is not a rendered-engine or live-handover
proof. The script exports complete-capture metrics, strongest peaks and final
state, without writing PCM. Python3.14.2/NumPy2.4.0 completed all12 profiles;
execution success is explicitly separate from candidate rejection. Previous
Live/Stretcher/continuous-history failures and all full B5 gates remain open.

### Rejected Source-Segmented PCM Overlap Candidate — 2026-10-16

`uv run --no-sync python scripts/pitch_overlap_probe.py` evaluates a frozen
NumPy-only full-PCM mechanism, separate from production and Native. Unlike the
AR2 candidate above, local resampling transposes every source component. The
custom short-window waveform alignment follows the
[WSOLA principle](https://www.isca-archive.org/eurospeech_1993/roelands93_eurospeech.html);
this is not a validation of the paper's implementation or every overlap method.

Before measurement, the model fixes a periodic Hann of 192 output frames, hop 48
and a search bound of 192 original source frames. Shared stereo normalized correlation
selects integer analysis origins; exact score ties prefer the smallest source
lag, then the lower origin. Source-only first-difference cues use an eightfold
previous 512-frame mean, zero-padded history, a 1e-12 numerical floor and 240-frame
refractory period. The renderer receives only PCM, rate and semitones. Scoring
attack locations are never renderer inputs.

Each source segment begins at ceil(source-origin/r), is locally floor/ZOH
resampled with slope h, then waveform-aligned to its canonical duration. Four
virtual initial windows are pinned to the segment origin; later windows cannot
reread its first original source tap. All admitted positive-time window tails
remain, even after the next segment starts. Static window-weight normalization
is per segment; summed segments receive no final normalization, fitted gain,
offset or nominal-duration crop. Finite grain retirement is distinct from nominal
material duration. Whole-source cue knowledge and correlation are offline
lookahead, not a proof of streaming causality or callback suitability.

The first run executes 32 profiles: original impulse, one-second 900.37-Hz mono,
331.37/997.19-Hz mixture plus impulse, existing eight-ms 900-Hz and percussion
source equations, nearby isolated impulses, running 241.13/613.71-Hz background,
and the same background with impulses at source 32768/33536. Each source uses
r=1/k=0 identity and r=2/k=-7,0,+7. Source length is 161538 frames at 48 kHz;
the two r=2 attack targets remain 16384/16768. Both channels share every cue,
grain origin and tap; right is half left.

Identity reconstruction through the same segmented overlap path has maximum
error 2.22e-16 against the fixed 1e-9 limit. Three source-frequency calibrations
are within .003804 cent. The centered 25-frame stereo energy formula matches an
independent explicit-window scalar control exactly, including impulse quantiles
H-10/H/H+10. This is a Python formula/control check, not fresh Rust execution.
Every generated sample is captured, followed by 20 ms of exact zero after all
admitted grains retire. All 32 profiles preserve the stereo relation exactly.

The original impulse, nearby isolated pair and percussion pass the bounded
two-ms timing, original peak, pre-target retention and retired-tail predicates.
The higher two-tone mixture also passes 15 cents at each pitch: maximum errors
8.5542/4.2239/1.6721 cents for k=-7/0/+7. These results do not certify waveform,
amplitude or general musical quality. In particular, floor/ZOH duplicates the
impulse's first source tap at k=-7, doubling its discrete output energy; no gain
correction hides this.

| Unchanged failed predicate | Actual result |
| --- | --- |
| Eight-ms 900-Hz attack, r2/k-7, q10/q50 | +85/+122 frames; q50=2.5417 ms exceeds 2 ms |
| Sustained 900.37-Hz source, r2/k-7, q10/q50 | +67/+100 frames; q50=2.0833 ms exceeds 2 ms |
| Running 241.13/613.71-Hz background, r2/k-7 | Actual strongest 156.6888/413.4909 Hz; maximum 46.2914 cents exceeds 15 |
| Same background, r2/k0 | Actual strongest 237.3510/615.1466 Hz; maximum 27.3468 cents exceeds 15 |

The paired busy source and control are bitwise equal before the first source
impulse, yet their complete pre-H output differs. Maximum differences are
.000618991/.016724686/.039321985 for k=-7/0/+7, affecting 28/220/50 sample values.
Cue-dependent segmentation and nonlinear correlation therefore cannot be treated
as an isolated linear attack response. Mixture prefix energy includes legitimate
background and supplies no isolated retention certificate.

Operational execution succeeds, but this frozen candidate is rejected. Its
full-PCM processing removes the earlier AR2 residual's systematic 2f error; it
does not resolve polyphonic phase, short-attack envelope timing, source-phase
quantization, aliasing, boundary gain or early background dependence. Simply
enlarging windows or tuning search constants to these failures would not prove
the required joint behavior. Previous Live/Stretcher negatives, finite live
handover, variable-map/full-pitch-range, continuous-history, realtime/resource,
device and Human gates remain open; no production renderer is activated.

## Settings Contract

Project persistence stores the global `key_lock` boolean as default intent and
`key_lock_intent_version: 2` with the fixed-length `pad_key_lock` override list.
Its strict boolean entries mean explicit ON/OFF; `null` means inherit. Loaded
legacy booleans retain their explicit values because their original provenance
cannot be recovered. Missing legacy values inherit; unloaded entries clear to null.
Requested intent is distinct from actual Native acceptance. It does not store Rubber
Band handles, DLL/shared-library paths, runtime buffers, measured latency,
algorithmic delay, or callback-internal backend state.

The performer Settings UI exposes no Rubber Band backend tuning surface. Rust
uses a maximum tempo-ratio step of `0.05` every `512` active output frames in
dry and Key Lock modes. A newly accepted target initiates its first step
immediately. Rendering splits at later step boundaries so source ratios remain
independent of callback partitions; paused output does not consume an interval.
Native pitch-update order also matches with equivalent initialized native/preparation
state and reserve availability. Missing reserves retain bounded silence without
changing the canonical source feed. These values are not persisted or user-tunable.

## Realtime Constraints

The audio callback must not:

- allocate or resize DSP buffers,
- read files or decode audio,
- load plugins or models,
- log,
- block on locks or waits,
- acquire the Python GIL,
- run neural inference or stem separation,
- spin while waiting for Rubber Band output.

The callback updates scalar mode/ratio state, reads bounded per-pad Key Lock
state, reads prepared source buffers, uses fixed Rubber Band staging storage,
consumes or produces bounded FIFO data, splits at exact prepared-adoption frames,
and mixes the resulting output through Gain/Trim, DSP, metering and master volume.
Source-specific priming/catch-up, native reset/cold pitch preparation and prepared
native/FIFO/PCM-owner destruction stay on the preparation worker.

## Native Dependency

The backend requires a Rubber Band C API that exports `rubberband_live_*`
symbols. The Windows vcpkg Rubber Band 4.0.0 package satisfies that requirement.
Ubuntu 24.04 `librubberband-dev` 3.3.0 does not provide the required LiveShifter
C API.

Build discovery uses documented platform mechanisms and explicit environment
overrides:

- Linux: `pkg-config` for a Rubber Band package with LiveShifter C API support,
  or explicit `RUBBERBAND_LIB_DIR` and `RUBBERBAND_INCLUDE_DIR` overrides.
- Windows: `RUBBERBAND_LIB_DIR`, `VCPKG_ROOT`, or the documented
  `%LOCALAPPDATA%\vcpkg` development location.
- Runtime DLL/shared-library availability is established before the audio engine
  enters realtime callback rendering.

The Windows source-run helper registers Rubber Band DLL directories before
loading the native extension. Standalone Rust test binaries do not import that
Python wrapper, so Windows Rust validation should either run with the Rubber
Band runtime directory already on `PATH` or use `scripts/run-rust-tests.ps1`,
which discovers uv's selected Python runtime plus the same documented Rubber
Band runtime locations and prepends them to `PATH` before invoking Cargo.
Packaging should provide the required runtime libraries with the application
artifact and account for Rubber Band licensing before binary distribution.

Reference URLs:

- https://breakfastquay.com/rubberband/integration.html

## Durable key intent and engineering pitch controls

K-META implements `key_intent.py` and the saved fixed-216 `pad_key_intent` table:
immutable source versions, independent correction/analysis epochs, numeric
`base_shift` (-5..+6 with fixed +6 tie), all 37 `extra_shift` values (-18..+18)
and the retrigger flag.
Legacy `manual_key` delegates to this same table. Its three resets are independent;
unknown source text remains display metadata without guessed root or mode.
Failed deliberate analysis admission preserves correction; a genuinely admitted
request can remove its prior correction, while later corrections survive its
guarded result. Accepted replacement resets source-bound intent; restore retains
it. New explicit correction `None` prevents legacy resurrection on reopen and
malformed new fields recover locally without discarding valid performer settings.

The implemented left-sidebar Key + Transposition surface now routes GUI choices
and learned MIDI/keyboard SET or SET_RETRIGGER actions through the same Native
`CapturedPitchAction` and Python `PitchController`. Absolute Key retains the
recognized source's major/minor mode; unknown source labels require correction
before an absolute choice. All 37 numeric extra shifts and the independent base/
extra resets remain available. Correction relabels without retuning or changing
position. The Retrigger checkbox affects GUI choices and newly learned variants;
existing bindings retain their stored variant.

SET preserves the source cursor and stopped state. Every admitted SET_RETRIGGER,
including repeated identical choices, retains its own content/lifetime, input
timestamp and original dueframe T. Source-specific preparation selects execution
frame E through the existing scheduler; tuple application and attack occur
together. Requested saved intent and pending/rejected/effective observations are
separate; a numeric choice alone does not confirm its audible effect.

Base and extra combine once into 48 total semitone values k=-23..+24, with
h=2^(k/12). The production pitch-intent path applies one pitch pass over the
existing varispeed feed: lockON p=h/r; lockOFF p=h, preserving speed coupling r*h.
It validates the exact finite positive value without the legacy .5..2 compensation
clamp. For r=.5..2, the joint requested domain is p~.132433..8; native preparation
uses that selected pitch and its source-specific history requirement. Numeric
validity does not replace window/feed/history/epoch or endpoint admission. Failed
preparation and unavailable contexts remain explicit refusals with previous
effective audio retained, rather than silently clamping pitch or changing tempo.
No cascade or complete transformed pitch PCM is introduced.

Moving SET now retains a bounded journal of actual successful source reads,
including dry reads: 1024 scalar spans and at most 262,144 active frames per voice.
Start/seek/restart/Stop fence it, Pause advances no frames, and failed reads form
explicit gaps. Each accepted SET copies the available suffix into its own
preallocated snapshot; the existing worker never reads mutable callback storage.
It replays genuine rate/read-plan epochs and the frozen F-to-E projection. Old
stem descriptors include copied complete PCM identities and original publication
generations; current held readers must cover the actual historical contexts.
Historical markers supply data provenance, with fresh strong endpoint authority
still required at adoption. Constant endpoint reuse first checks the real required
read/selection chronology, including gaps and completed selection changes.
SET and prospective mode replay can resolve selection history through one actually
held committed stem set, including FullMix/stem ramps, changed masks and an empty
mask. Genuine-history requests may retain a ramp still active at original E.
Each sliced plan
determines its required reader roles before identity/coverage resolution; missing
roles cannot disappear through a failed lookup. FullMix E retains the physical
stem pin needed by earlier reads or by the outgoing endpoint role. Both logical
reader roles receive independent physical coverage and exact adoption checks,
including when they alias one set or select an empty mask. Other/unheld sets and
stale publication generations still refuse only that request and retain effective audio.

An active-E prepared Native owner carries a scalar expected-transition state,
initialized only after genuine replay reaches E. Each actual successful source
chunk checks its exact pre-read phase and advances it by the same actual source
progress as the mixer. Pause contributes no frames. Natural completion retains
the same Native/FIFO/filter owner and a completed tracker; mismatched progress,
premature clearing and a restarted ramp invalidate that continuation proof rather
than becoming valid again after another clear. No-history/automatic preparation
still refuses active ramps, and active-E replay cannot use backward constant
reconstruction. This extends finite-history engineering, not continuous-O or
acoustic/deadline acceptance.

Selection ramps use the same canonical rate-epoch progression on the live reader,
snapshot slices, endpoint projection and preparation worker. An epoch retains its
source-progress anchor, applied ratio and integer output-frame count. Both ramp
gains and boundary state evaluate that anchor plus the complete epoch count times
ratio, so a 17-frame projection and callbacks of 3 then 14 frames are identical.
Only a real applied-rate change reanchors; zero-frame calls leave the epoch intact.
Strict adoption compares that complete state without a numerical tolerance.

Already admitted immutable stem data keeps its copied original publication
owner/expected identity across a storage-only resident window change.
Initial stem admission still requires its full original source/timing/window
permit. A fresh Native action independently requires its own current window and
timing ACK; an adopted Native owner uses its existing effective authority. Storage
relocation therefore preserves productive Native/FIFO/filter continuity without
letting an old strict action adopt against a newer window.
The same publication may also be a mutable selection-operation receipt. Ordinary
Play can withdraw that observation, and analysis/timing requests can supersede
request epochs, while the admitted physical PCM remains valid. Those operation
checks apply to initial admission; they cannot revoke retained data independently
of the owning fresh/effective Native source, timing and voice authority.
An independent selection receipt reusing actual admitted readers preserves their
original data stamp. Shared-cache reconstruction alone has no prior admission;
its placeholder publication takes the new operation's genuine admitted stamp.
Four public SET cases and eight prospective mode cases compare all 4,097 adopted
samples with independent scalar/direct-C-ABI A-reset references at +24/-23,
with exact phase/FIFO and stable Native ownership. Their observed maximum PCM
error is zero; deliberately replacing the past with FullMix disagrees by at least
0.228. Public selection/source ABA refuses the original capture and a fresh
request succeeds on the same worker. A paused mid-ramp endpoint still refuses.
Slice tests cover before/at/after ramp completion, empty masks and missing,
different or republished owners. Tracked publication/WindowACK tests preserve
productive relocation PCM and accept a fresh own-window request while refusing
the old strict request, changed held publication identities and actual Native
source/timing revocations. These
checks establish the bounded replay and ownership contract, without closing
continuous-from-birth, acoustic or performer-deadline acceptance.
The real Controller stopped-ALL/ordinary-Play/explicit-ALL path preserves its
admitted data stamp and productive owner through separate receipts. A two-pad
preload probe also inspects actual queued adoption: both pins start absent,
the second reuses the first's complete/component Arc owners, and each receives
its own pending then accepted/current data-publication stamp. This proves shared
reader admission separately from the existing full 216-pad and P6 obligations.
Source/rate/domain/selection edits and Pause/Resume fence each captured F-to-E
projection without discarding its genuine past. This source-trajectory epoch is
separate from Stretch's DSP preparation epoch: a later prospective Key Lock
preparation cannot cancel an earlier SET on an unchanged source. A later tuple
using the old numeric pitch still fails its own captured-pitch check after that
SET applies at its original E. Restoring a changed rate before E cannot hide the
intervening source edit.

Paused nonstarting SET captures that same action-owned genuine history, ending
at active F with zero future active frames while retaining its original global E.
The existing worker and strong action permit prepare and adopt its own candidate;
Resume continues with that Native/FIFO and retained source/filter state. Silent
waiting time supplies no missing active past. Resume/Pause or restored rate edits
before E still invalidate the original source-trajectory capture.

Eight paused SET Intro/Tail variants at +24/-23 and both Lock modes compare each
of 12,290 resumed samples against the independent scalar/direct-C-ABI A-reset
reference, with exact source phase/FIFO and the same adopted Native owner beyond
the later 4,096-frame boundary. A cloned warm-versus-reset filter probe makes
retained filter state observable. Original sequence, timestamp, content and T/E
remain fixed. Resume/Pause and rate-restore races refuse the original capture;
insufficient variable past also refuses despite silent waiting, and each fresh
same-worker retry retains its own independent resumed PCM proof.

Prospective nonzero Key Lock changes use the same genuine-history replay. Each
source lane owns one additional reusable snapshot Box, allocated before rendering.
Capture reserves its storage and request capacity before holding the prospective
DSP tuple. The existing source worker projects the frozen checkpoint once to the
original E, replays actual past plus that future, then clears and returns the
large snapshot through bounded recycling. Success, stale work, unavailable
history and failed-worker retirement retain this ownership discipline.

A small SOURCE/voice guard remains in the candidate's fresh Native permit after
snapshot recycling. It narrows strict adoption before and after fresh authority
checks; productive-history and physical endpoint observations retain their
existing semantics. Mode supersession continues to use Stretch's separate
preparation epoch. All participating voices and the original finite resident
ticket still commit together at E.

Paused prospective participants keep that global E but project zero active source
frames between capture and adoption. Their candidate survives paused render polls;
the callback services returned storage without consuming its Native/FIFO. The
claim checks the original paused state, so a Resume before E refuses that request.
After adoption, Resume retains its source position, active smoothing clock, filter
history and prepared Native/FIFO. Source authority uses the existing actual current
cell epoch or freshly captured retained voice birth, while the separate lane epoch
and SOURCE guard continue to fence pending work. Paused ON is armed; adoption clears
that voice's actual wet-output flags until its own resumed output.

Public Intro/Tail tests cover both mode directions at +24 and -23 semitones after
off-grid rate edits. Each compares all 4,097 adopted samples, source phase and
FIFO against an independent direct-C-ABI A-reset reference. Restored-source-control
refusal and a fresh request use the same lane. Lane ownership and permit tests
cover snapshot reuse, stale work and bounded return saturation. The additional
32 mode snapshot payloads now total 35,389,440 bytes before owner and queue overhead;
these structural counts do not establish total process memory or a deadline.
An explicit Release profile filled all 1,024 spans in each of 32 mode snapshots:
128 measured capture batches after eight warmups took 1,313.3 microseconds minimum,
1,415.5 median, 1,564.5 p99 and 1,572.7 maximum on this host. Timing includes the fixed
future assignment, excludes clearing, guards, projection, queues and Native replay,
and establishes no callback deadline. Each scalar/optional span is 1,080 bytes;
one 1,024-span allocation is 1,105,920 bytes. The profile actually allocates 32
journals and 32 mode snapshots (70,778,880 span bytes). Adding the 64 admitted SET
work owners derives 141,557,760 span bytes. The canonical transition epoch adds
16 bytes per span and 2,097,152 bytes across those 128 span allocations compared
with the earlier selection-history layout. Request inline storage is 2,384 bytes
and its preparation enum is 2,400 bytes; those are separate from span payloads.
Allocator, Arc, queue, Native and PCM storage are separate. These measurements
replace the earlier layout and host timing profile; they are no whole-process
resource or callback-deadline proof.

Eight paused Intro/Tail variants at +24/-23 compare each of 12,290 resumed samples,
phase and FIFO against the independent A-reset reference with no paused future
reads. They retain the claimed Native address and queue no automatic replacement,
including after the later 4,096-frame boundary. Real filter-history probes distinguish
warm and reset state and stay unchanged through pause, adoption and Resume. Public
Controller +7/-3 GLOBAL tests retain separate resident tickets and E values one
frame apart for running and paused targets, with independent explicit-peer PCM/EQ
checks and armed versus first actual wet feedback. Refusal/retry retains original
receipts across Resume/Pause, supersession and Stop.

Prepared ordinary local and GLOBAL births rearm the existing zero-pitch ON
first-live-wet guard after adopting their clean candidate. A previous voice's
successful wet output cannot certify a fresh start after STOP. The first live
failure retains dry source chronology and reports OFF/error; success consumes the
already adopted Native/FIFO without another reset or exchange. A still-current
source/window/MODE acknowledgement may be reused, while the new captured START's
own sequence, timestamp, original E and adopted Native request remain distinct.

Variable replay declares a fresh reset A on the voice's active-frame origin O's
512-frame lattice. H queries configure each actual pitch before measuring its
reset-dependent bound. Alignment can add up to 511 frames and must fit the full
retention budget. The cold profile uses the pitch of the first completed real
Native block; later spans do not reset Native. Independent Intro/Tail tests at
k=+24 compare every adopted sample, phase and FIFO against this declared A-reset
contract, and reject a backwards-constant reference. Public insufficient-history
refusal preserves the original action and old PCM, with a later valid action
using the same worker.

This bounded nonzero-pitch production path remains an engineering delivery.
A-reset provenance does not prove equality to Native continuous from voice birth:
an off-grid 48 kHz/k=+24 probe retained identical FIFO counts but observed maximum
PCM differences of 0.30 to 0.48 even with genuine aligned suffixes of 4,205 to
49,261 frames.
That negative equivalence result supplies no acoustic verdict; the original
quality gates remain required. Broader variable history and pause/selection
coverage, active selection transitions, performer deadlines, the full B5 quality/latency/
unity-transition matrix, B6 and Human app/device/listening/visual acceptance remain
open. R2 layout/hold and B7 captured pitch+retrigger retain their prepared ownership
guards. Neither this surface nor persisted K-META completes whole-arrangement,
full-216 or final product acceptance.

## Direct live KEYLOCK correction2026-10-11

[E11-01/02](pre-rust-extension-20261011.md) have the bounded X11-KEYLOCK software
correction for fixed NormalLoop k=0 through actual global/pad command, current-window/native ACK
and wet Native/FIFO/filter output at nonunity SPEED/BPM-lock. The latest inheritance contract
supersedes broadcast overwrite: explicit ON/OFF survives later globals, new tracks inherit and
the adjacent GLOBAL action clears only the local override. Pending/error/confirmed audio remain
separate from origin colors. Hardware-free production-path, controller and draw tests cover this
correction; accepted lifecycle proof remains a narrow prerequisite. Remaining whole
P4b/P5a/nonzero B5 and Human app/device/visual/hearing gates remain OPEN.

The correction keeps mode/window ACK, initial wet processing through an already warmed unique handle, and later exact source-candidate adoption separate. Initial wet output need not await that later4096-frame candidate; WindowACK alone does not prove source-specific adoption. Actual current SourcePlayback/feed and matching history permits remain authoritative through pause/resume and old voices.

## Inheritance validation2026-10-11

The completed inheritance block uses version-2 nullable overrides and one resolver
for global/pad intent, persistence, load completion and residency. Successful loads
retire previous-source residency state and force a source-bound publication, including
timing-stale loads. OFF during Native-only waiting/error cannot be mistaken for a
settled dry mode. Failed replacement preserves current loop geometry and requested
override origin. The sidebar draws requested origin separately from confirmed audio.

Validation on local main, with no Rust production or ABI change:

- Integrated controller/UI/model suites: 1,907 passed, 20 skipped. After the final
  review fixes, the affected transport/load/timing-retirement/UI suites were rerun:
  1,025 passed, 2 skipped. Unaffected persistence/model results were reused.
- Actual Python-controller/Native worker tests: 10 Debug tests passed before the
  final waiting-OFF regression; all 11 tests passed in Release afterward. These
  exercise real nonunity wet/dry output, phase, pending failure, reload and all banks.
- Ruff, Mypy (255 files), Cargo format/check and Release startup preflight passed.
  The final preflight imported the existing Release extension without starting the
  Human App. No installed `.pyd` or private settings/samples were replaced.
- Official strict validation passed for `inherit-pad-key-lock` and the affected
  `adopt-pad-owned-pcm-residency` change. Git whitespace checks passed.
- One independent final review found two concrete lifecycle issues; both were fixed,
  regression-tested and closed by the same reviewer. No findings remain in that scope.

Initial regressions against old broadcast/reset expectations failed and were updated
to the new specified behavior; an intermediate UI fixture range was corrected.
Existing torchaudio and analysis compiler warnings remain. The skips and these
software checks grant no App/device/hearing acceptance and close no full P4b/P5a,
nonzero-pitch or whole-program gate.

## Prepared endpoint resource probe — 2026-10-15

The opt-in `pitch_reserve_resource_tests` probe retains distinct actual Native
handles, adapters and source-specific input/output FIFOs in fresh Windows child
processes. It uses one generated 48 kHz stereo source of 65,536 bytes, allocated
before the baseline. Each of four profiles measures 1, 8, 32 and 64 retained
bundles; all 16 children verify exact selected pitch, history length, source
ownership and available source taps. This measures preparation costs without an
App, device, installed extension or complete transformed PCM cache.

| Profile | Native p | History frames | Private commit delta, 64 bundles | Native construction | History preparation |
| --- | ---: | ---: | ---: | ---: | ---: |
| Unity calibration | 1 | 4,096 | 149.13 MiB | 34.15 ms | 227.53 ms |
| Saved lock, r=.73 | 1.369863 | 4,096 | 149.13 MiB | 33.69 ms | 290.04 ms |
| Total -23, r=2 | .132433 | 29,184 | 149.13 MiB | 33.99 ms | 1,402.89 ms |
| Total +24, r=.5 | 8 | 23,552 | 149.05 MiB | 34.24 ms | 1,209.76 ms |

Times cover all 64 bundles sequentially in this Debug probe. They are observations,
not callback budgets or Release performance guarantees. The retained working-set
delta is 141.76–143.01 MiB. Releasing all 64 bundles reduces private commit by
146.59–146.66 MiB; 2.46–2.49 MiB remains above the process baseline. This single
release observation does not establish repeated-cycle leak freedom.

The first run queried history length before selecting the actual Native pitch;
its lowest-pitch expectation failed at 18,944 versus 29,184 actual frames. The
probe now queries the selected state and keeps strict history equality. Neither
these resource measurements nor an idle prepared endpoint resolves the existing
1,598-frame pre-target retention conflict. Unique 216-pad sources, the P6 matrix,
performer deadlines and acoustic/Human acceptance remain open.

Pitch publication now reserves a separate engine-local limit of 64 work owners
after checking the command queue and before allocating work/history backing.
Each claim remains with that original work through queued, preparing, ready,
scheduled, cancelled and retiring states until actual off-callback destruction.
Retaining a receipt does not retain retired work capacity. Callback capture keeps
its preallocated history storage; Native/adapter/FIFO construction occurs only
on the existing worker after fresh original source/permit checks. Stopped SET,
actual scalar/no-op and pre-preparation rejection construct no Native. Format
errors still reject synchronously; Native construction failure is a local
asynchronous preparation refusal.

Adoption swaps the prepared adapter into an existing voice and returns its
displaced adapter through the same work for retirement. The 64 work-held
snapshots/adapters and existing MAX_VOICES productive adapters are separate bounds;
ordinary starts, mode work, endpoint reserve and other owners are additional.
The historical resource table above is not a measurement of this new custody
path and neither bound certifies total RSS, P6, 216 unique sources or a deadline.
