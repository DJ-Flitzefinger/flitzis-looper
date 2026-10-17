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

Effective stem-mode acknowledgement uses the same combined piano-pitch and
matching Native-history wet decision as rendering. Nonneutral piano pitch at
source rate 1 still needs the new selection's own current-source wet output with
KEY LOCK OFF or ON. A fresh Native-unity birth can remain dry, while an existing
matching Native owner retains its aligned history at unity. Completing the
128-source-frame ramp alone does not certify that wet output. The bounded
[public Generate profiles](prepared-stem-publication.md#piano-pitch-and-live-generate-mode-acknowledgement--2026-10-17)
retain the existing own preparation/adoption path and its separate acceptance limits.

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
The original run captured every generated sample and then appended 20 ms of
silence. Its reported final slice therefore did not independently test tail
energy. The shared scorer now measures the renderer's actual final 20 ms before
any added silence; a deliberately nonzero final sample guards this distinction.
All 32 original profiles preserve the stereo relation exactly.
The affected 32-profile rerun retains the same timing and tonal negatives.
Its running-background final slice reaches 1.183381% energy; that slice includes
legitimate ending material and is not an isolated-response settling certificate.

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

### Centered Full-Phase-Gradient Diagnostic — 2026-10-16

`uv run --no-sync python scripts/pitch_phase_gradient_probe.py` compares a full
phase-gradient candidate with the unchanged sources and scoring from the overlap
probe. Its basis is Prusa and Holighaus,
[Phase Vocoder Done Right, sections II–III](https://www.eurasip.org/Proceedings/Eusipco/Eusipco2017/papers/1570343436.pdf)
and the authors' [phase-vocoder implementation](https://raw.githubusercontent.com/ltfat/phaseret/main/sigproc/phasevocoder.m).
Both temporal instantaneous frequency and frequency-direction group delay guide
phase propagation. Magnitude-priority integration can follow separate spectral
components instead of requiring one waveform lag for an entire overlap.

This bounded implementation fixes a centered periodic Hann/FFT of 4096 frames
and analysis step 256. Analysis centers A and synthesis centers B=floor(alpha*A)
have explicit centered phase gauges, with alpha=h/r. Their fractional center
d=B-alpha*A participates in both gradient directions. Every synthesis packet is
captured on its complete signed timeline. Negative-time samples and pre-target
energy remain in the report; the predetermined array origin changes no target.
Static finite dual weights depend on admitted windows, never on wet values.
Only 0<alpha<=1 is admitted in this first candidate; larger factors are explicit
unsupported inputs and remain required future gates.

The shared stereo multiplier is derived from pooled channel phase connections;
source interchannel spectral phase is preserved. A coefficient priority threshold
of 1e-6 selects integration work, while all magnitudes remain. Endpoint multipliers
are common real signs. Empty connections, source-based initialization and heap
ties are deterministic. This stereo extension and finite wrapper are specific
to this diagnostic and do not inherit a general quality guarantee from the paper.

The finite resampler uses a Kaiser-windowed sinc, following the
[bandlimited interpolation method described by Julius O. Smith](https://www.dsprelated.com/freebooks/pasp/Windowed_Sinc_Interpolation.html).
Radius 32, beta 8.6, rate-dependent cutoff and static kernel DC normalization are
fixed before measurement. Its causal shift has physical group delay 32/h output
frames, retained in the acoustic timing result. h=1 uses the exact copy conversion;
live continuity across that branch remains unproved. All finite filter tails are
captured. No fitted offset, wet gain or nominal-duration crop is applied.

Independent analytic gradient controls, unchanged-coefficient unity reconstruction,
the original centered energy metric and a deliberate negative-time-energy rejection
precede quality scoring. The matrix retains all eight previous sources and adds
the eight-ms tonal attack over the matched running background. Unity reconstruction
does not test heap integration because unity explicitly preserves coefficients.
No Native, callback, production renderer, device or Human acceptance is involved.

The final run completes 36 profiles with every tested sustained pitch within
15 cents; the largest error is .06757 cent on the lower two-tone background.
Nine unchanged-coefficient identities have maximum error 4.44e-16. Analytic
gradient/rounded-edge errors are at most 3.18e-13, and the small finite dual
reconstructs first/last samples exactly. These controls establish their stated
algebra and coordinate invariants, not general heap quality.

The two-ms timing of the short tone improves, but the candidate remains rejected:

| Original isolated response | r2/k-7 | r2/k0 | r2/k+7 |
| --- | --- | --- | --- |
| Eight-ms 900-Hz q10/q50 offsets | -19/+48 frames | -33/-1 frames | +10/+22 frames |
| Same attack, energy before H | 3.827129% FAIL | 3.768322% FAIL | .007011% |
| Eight-ms percussion, energy before H | .532876% FAIL | 10.557129% FAIL | .030079% |
| Nearby isolated pair, energy before H | .521740% FAIL | .347312% FAIL | .173346% FAIL |

The pre-target limit remains .1%. The nearby pair also fails its independent
two-peak timing check at each shifted rate2 profile. Sustained mono/poly sources
at k=-7/0 retain further timing or pre-target failures. A passing single impulse
does not establish their response: its r2 q50 offsets are +48/0/+21 frames,
including the actual finite SRC delay.

The complete signed output intervals at r2 are [-4093,85083), [-3072,83968),
and [-2391,83329) for k=-7/0/+7; nominal material duration remains 80769 frames.
Actual renderer-end tail fractions are at most 2.20e-8, below 1e-6. This result
uses the full retained finite renderer output, without an artificial silent
slice or an IIR/native settling claim. For the busy short-tone/control pair,
pre-H maximum PCM differences are .198771/.233195/.025978. They are nonlinear
mixture differences, not isolated attack responses or retention certificates.

The single independent review found and closed two diagnostic defects: appended
silence had made tail scoring vacuous, and endpoint real-sign projection happened
after heap propagation. Endpoint corrections now enter current and next-frame
state with their emitted real sign. Both regression controls failed before their
fixes; an actual sign-changing DC sequence also checks propagated neighbor and
next-frame phases. The affected 36-profile run was repeated with unchanged DSP
parameters and thresholds. Earlier negative results remain recorded.

This narrows the unresolved mechanism: full-gradient phase propagation repairs
the measured tonal errors, while transient energy still anticipates the original
target. No gate is relaxed or response cropped. Full stretch range, variable maps,
live handover, realtime/resource and Human gates remain open.

### Complete Causal Subband AM/FM Diagnostic — 2026-10-16

`uv run --no-sync python scripts/pitch_amfm_probe.py` evaluates an independent
finite-bank construction. This is an original diagnostic, not an implementation
of the published muTVS method. The [author's muTVS demo](https://neerajww.github.io/preprint/demo/tsm/tsm_demo.html)
was read; its full paper equations/code were not obtained. The primary
[Quatieri/Hanna filterbank summary](https://www.ll.mit.edu/r-d/publications/perfect-reconstruction-time-scaling-filterbanks)
expressly makes elemental reconstruction conditional on the bank and requires
additional signal-dependent conditions for mixtures. Neither source proves the
project's original 2-ms, .1%-retention or 15-cent requirements for this construction.

Before measurement, the diagnostic fixes a nondecimated causal bank with N=1024,
lags l=0..1023, w[l]=(.5+.5*cos(pi*l/1023)), w[0]=1 and w[1023]=0.
For k=0..512, g_k[l]=d_k*w[l]*exp(i*2*pi*k*l/1024)/1024, where endpoint d=1
and interior d=2. Summing the real kernels gives delta[l] on this support, so
Re(sum_k x*g_k)=x. DC and Nyquist participate; there is no omitted complement or
unchanged residual. These finite causal filters are not strictly analytic.

The source-only segment detector is reused unchanged from the overlap diagnostic:
stereo first-difference norm exceeds eight times its zero-padded past-512 mean
and 1e-12, with a 240-frame refractory period. Disjoint source segments sum to the
original PCM. Each segment is independently decomposed, and every generated band
tail survives beyond its segment and overlaps later streams. Trimming exact zero
input support is an algebraic convolution optimization, not a measured output crop.
Offline access to the complete source is explicit; this is no live-history proof.

For each band, pooled stereo temporal complex products provide a common phase
increment unwrapped around its bin carrier. Exact pooled cancellation falls back
to the largest individual channel product, with left ties. A zero product uses
the bin carrier; exact common-zero runs have explicit retirement/birth semantics.
The birth gauge comes from the largest birth coefficient, with left ties.
Both channels receive the same correction (h/r-1)*(phi(s)-phi_birth), at original
source coordinate s=r*n-birth. Thus each band's envelope follows r and its common
carrier follows h. Complex coefficients and unwrapped reference phase use fixed
linear interpolation at fractional coordinates. Integer-coordinate unity and
fractional rate-only h=r reconstruction are distinct controls; interpolation is
not exact arbitrary-band analytic resynthesis or antialiasing. Zero-extended
linear interpolation retains its full support, including up to one source frame
of fractional preview before each component birth. Any negative-time output is
captured with its declared origin; integer-coordinate r=1/r=2 has no preview.

The first matrix retains the nine phase-gradient sources and adds independently
gated 900/1100-Hz carriers plus a later impulse. Identity is measured first, then
r=2/k=-7,0,+7, for 40 complete responses. Integrity controls precede quality
measurement. Every original pre-target/timing/peak/tail/frequency threshold remains
unchanged, and the renderer receives no attack targets. Causal image leakage,
phase ambiguity at cancellations/endpoints, source cue chopping, aliasing and
general mixture quality are explicit risks. Results below are recorded only after
actual execution; no Native, install, App, device or hearing gate is claimed.

The first complete run executed all 40 profiles in 248.246 seconds and rejects
the candidate. No DSP parameter, source, target or threshold was adjusted after
measurement. All 16 sustained tonal profiles pass 15 cents, with a maximum error
of .437342 cent. All ten unity profiles and every pre-warp bank reconstruction
remain within 7.11e-15 of the original. Independent explicit FIR summation, direct
convolution and FFT comparisons pass; a known nonunity phase control is within
1.75e-15. A 1034-frame structural zero region contains exactly zero coefficients
and produces exactly two phase runs. Signed fractional rate-only controls at
r=h=.5 and r=h=2^(7/12) match independently interpolated PCM within 1.78e-15.

At source-rate2, the unchanged q10/q50 offsets are:

| Source | k=-7, frames | k=0, frames | k=+7, frames |
| --- | --- | --- | --- |
| Original eight-ms 900-Hz attack | +15/+21 | +9/+13 | +5/+10 |
| Original eight-ms percussion | +4/+8 | +2/+4 | +1/+2 |
| Nearby isolated impulse pair | +1/0 | 0/0 | 0/0 |
| Original 331.37/997.19-Hz pair plus impulse | -23/-130 FAIL | +123/+233 FAIL | -64/-75 |
| Independently gated 900/1100-Hz carriers plus impulse | -948/-684 FAIL | -1249/-481 FAIL | -928/-790 FAIL |

The original limit is 96 frames for both q10 and q50. All isolated profiles have
zero pre-target energy and preserve the original peak-after-target predicate.
The nearby pair's two strongest peaks retain their independent target timing at
all three shifted profiles. Isolated actual renderer-end tail energy is zero:
each finite response is retained inside the original source's trailing silence.
The running background reaches the source end and its actual final interval
contains remaining music (maximum .607275%); this is not an isolated decay proof.
Busy/control pre-target differences are at most 2.992e-14, consistent with FFT
roundoff in this bounded matrix, not a general floating-point causality proof.
Stereo R=.5L error is zero throughout the matrix.

Accurate frequency and short-attack timing do not establish acceptable gain or
envelope shape. The mono 900.37-Hz output/source energy ratios at r2 are
2.420940/1.930568/1.129536 for k=-7/0/+7, rather than a duration-only ratio of .5.
The short 900-Hz attack reaches raw peaks 1.88481/1.81484/1.43382 from an original
bounded source. These gains were measured, not normalized away. The independently
gated mixture changes the energy distribution despite retaining both strongest
carrier frequencies. Complete analysis reconstruction therefore does not provide
the required transformed synthesis weighting/phase relationships. Negative-image
leakage, ambiguous mixed-band phase and birth-dependent interband alignment remain
concrete mechanisms to resolve; this result does not reject every AM/FM method.
Metadata counts shifted phase increments outside Nyquist without clamping or
amplitude weighting; those counts alone do not quantify audible alias energy.

No productive backend changes follow this rejected experiment. Full pitch/rate
range, gain/mixture quality, variable maps, continuous live history, realtime
preparation/resources and Human gates remain open.

### Fixed-Bank Synthesis Calibration — 2026-10-16

`uv run --no-sync python scripts/pitch_amfm_calibration.py` independently predicts
the rejected bank's raw output. The renderer and its 40-profile quality results
are unchanged. This control uses the original mono source's known carrier only
in analytic expectations; the renderer still receives PCM/rate/semitones alone.

The finite response is calculated as three geometric sums, with fixed
alpha=pi/1023 and a zero empty sum. Positive and negative frequency responses are
both retained. Direct convolution supplies the original 96-sample leading and
falling ramps; partial responses supply exactly the flat source interval
96..47903. All 49023 possible source-response coordinates, including full filter
retirement, enter the prediction. Independent state arguments plus explicit
integer winding reproduce the original carrier-anchored phase rule at its actual
first active source frame 1. No phase alignment, gain or response crop is fitted.

Closed-form versus explicit FIR responses agree within 1.91e-14; the real-bank
response identity is within 1.87e-14. Selected complete complex states agree with
direct original-PCM convolution within 5.05e-14, and their real sum reconstructs
the original source within 2.55e-13. The smallest nonzero band magnitude is
3.33e-15 and the minimum interior phase-branch margin is .000949913 radians.
The predictor explicitly retains 592208 integer winding steps. No band is
negative-image-dominated at this carrier; this does not mean image leakage is zero.

The first actual run completes with all three whole-PCM predictions below the
predeclared 1e-8 limit. Maximum raw PCM error is 5.39e-12 across all 80769 returned
stereo frames. Whole-output energy prediction errors are below 2.31e-12 relative.
These numbers include both ramps and complete finite retirement, rather than a
single-period or stationary-window energy estimate.

| k at r2 | Actual output/source energy | Sum of individual band energies | Signed cross-band energy |
| --- | --- | --- | --- |
| -7 | 2.420940 | 350.769741 | +4177.184176 |
| 0 | 1.930568 | 350.768429 | +3260.028670 |
| +7 | 1.129536 | 350.765722 | +1761.839027 |

The changing constructive interference accounts quantitatively for these raw
energy ratios. A deliberately incomplete surrogate using stationary analytic
phase with principal filter phase differs from actual fixed-window PCM by
.081860/.050785/.036574. It omits image/onset-winding effects and is not an
alternative passing renderer. The control does not assign every observed mixture
timing error to one cause.

A source-only single-mode recurrence on the maximum-energy 16384-frame source
window estimates 900.3699999999993 Hz, with relative recurrence residual 2.89e-13.
That estimate, not the oracle frequency, builds the stacked positive/negative
image matrix. Its condition number is 16.9963; recovered positive-mode phasors
agree with the known source within 1.11e-13, and the relative out-of-span norm is
1.96e-13. Applying this one-mode span to the original mixed 331.37/997.19-Hz source
leaves .980171 of its observed coefficient norm outside the span. That remainder
is reported in full; it has not been transformed by a new corrected renderer.

The bounded next candidate therefore needs source-derived modes with independent
envelopes and retained birth gauges, plus an explicitly transformed complete
remainder. A later unrelated impulse must not rebirth every continuing carrier.
Exact source reconstruction alone cannot choose a unique physical AM-versus-
sideband interpretation of arbitrary audio. Separation assumptions, unresolved
modes, conditioning, transient quality, aliasing, full pitch/rate range and all
live/realtime/Human gates remain required. This calibration activates no backend
and changes none of the five original timing failures.

### Independent Source Modes And Complete Residual — 2026-10-16

`uv run --no-sync python scripts/pitch_independent_modes_probe.py` evaluates
source-derived stationary carriers with independent envelopes. The source-only
decomposition is prepared once per fixture, without requested rate, pitch or
scorer targets. This is an offline NumPy diagnostic. The
[Hua/Sarkar matrix-pencil paper](https://www.math.ucdavis.edu/~saito/data/sonar/HuaSarkarPencilMethod.pdf)
provides the exponential-estimation framework; the stationarity admission,
stereo coherence, envelope fitting and complete-residual renderer here are custom
rules, not a claimed published music-separation method.

Before measurement, carrier estimation fixes 8192-frame windows, hop4096 and a
pooled-stereo 32-lag Hankel pencil with direct thin SVD. Retained singular values
exceed 1e-7 of the largest, with even rank2..12 and at most six carriers. Each
window requires an eight-block minimum/maximum energy ratio of at least .25 and
crest at most8. Poles require radius error at most .005, conjugate distance at
most1e-6, frequencies20..23520Hz, eigenvector condition at most1e7, source-design
condition at most1e6 and source relative RMS error at most1e-4. Clusters span at
most1Hz and require three distinct source windows. Source-only variable projection
uses at most six iterations, at most .5Hz per step, four trial line-search scales
requiring decreasing source error, and normal-matrix condition at most1e12. Fixed complex stereo coefficients
use source-zero phase coordinates and normalized inter-window distance at most1e-3.
Refused carriers remain in the exact source residual.

Joint nonnegative envelope fitting uses centered256-frame symmetric-Hann source
windows with hop64, unpenalized active-set least squares and SVD least-squares solves. Its
gradient tolerance is64*epsilon*norm(A)*norm(b), full-design condition limit1e6,
and passive-solve cap128 per knot. A refused tile contributes an explicit zero
knot and retains its entire error in the residual. Linear envelope interpolation
is masked only outside the original common source activity hull. Every mode
retains the first active source frame as its birth gauge; an unrelated impulse
or global bank cue does not reset it. Centered fitting needs128 source frames of
lookahead. Its nominal working arrays remain below256MiB in the diagnostic;
offline allocation estimates are not native resource or deadline measurements.

For analytic mode z and fixed source carrier omega, the transformed contribution
is Re(linearSample(z,s)*exp(i*(h/r-1)*omega*(s-birth))), with s=r*n. Both channels
share the phase correction. The exact complement e=x-Re(sum(z)) passes in full
through the previous complete AM/FM bank, including every finite tail. The same
zero-extended complex-linear sampling geometry preserves combined h=r identity,
including fractional preview. No residual stays dry and no output gain, phase,
delay or crop is fitted. A known admitted carrier with frequency*h>=Fs/2 rejects
the request explicitly; unresolved residual aliasing remains unproved.

The first matrix completes72 requested profiles in388.313 seconds:71 rendered
responses and one explicit near-Nyquist rejection. It keeps all original40 and
adds32 profiles with unseen frequencies,12.26Hz spacing, amplitudes, phases,
independent gates and impulse positions, chirp, AM, noise, unrelated stereo and
19873.23Hz. Original metric controls and nine source-frequency calibrations pass.
All18 unity responses are within7.11e-15 of PCM, complete source decomposition
within1.12e-16, and proportional-stereo error is zero. Every rendered isolated
response has zero pre-target energy, a peak after its first event, and zero energy
at the actual finite retirement interval. Fractional mode-plus-nonzero-residual
identity error is2.22e-16; independent nonunity mode phase error is1.70e-15.
Source-estimator controls on unseen independently gated modes are within1.60e-12Hz
and5.20e-12 normalized stereo-coefficient distance. NNLS optimum, singular refusal,
silence, nonnegative support and independent stereo-envelope controls pass.

The original five mixture-timing failures now pass the unchanged96-frame limit.
The sixteen original sustained tonal measurements are within .009750cent. At r2:

| Source | k=-7 q10/q50, frames | k=0 q10/q50 | k=+7 q10/q50 |
| --- | --- | --- | --- |
| Original331.37/997.19Hz plus impulse | -11/-14 | +7/-16 | -7/-14 |
| Independently gated900/1100Hz plus impulse | -11/+22 | -2/+25 | -2/+51 |
| Unseen427.43/1327.89Hz plus impulse | +4/+10 | -5/-1 | -5/-4 |
| Unseen371.39/1381.77Hz with independent gates | +2/+5 | -8/-3 | +9/+3 |

Mono900.37Hz output/source energy is approximately .5 at each shifted profile,
removing the previously calibrated full-bank gain for this admitted mode. The
original eight-ms attacks are too short for carrier admission and retain the
unchanged complete-bank response, including raw900Hz peaks up to1.88481. Passing
the recorded timing and frequency predicates does not certify their gain quality.

The candidate is rejected overall. Eleven unseen timing profiles fail: all three
close-pair, chirp and noise profiles, and AM at -7/+7. Close-pair q10/q50 offsets
are -1108/-2141, -901/-2029 and -1006/-1952 frames; tonal errors83.29/41.22/13.71cent
fail15cent at -7/0. All13 nonzero close-pair windows are refused by the energy/crest
guard before a pencil is attempted: stationary two-tone beating is not equivalent
to nonstationary carrier frequency. No claim about a measured rank failure follows.
All close-pair PCM therefore reaches the previous residual renderer. Chirp and
noise also admit no stationary modes. Their q10/q50 offsets are +224/+431,
+145/+269,-106/-231 and +212/+222,+135/+102,-222/-236 respectively. The AM source
admits three stationary sidebands and fails -7/+7 at +442/+382 and +210/-25 frames.
A single physically modulated carrier and three sidebands are different valid
interpretations of the same PCM; this run proves neither a unique interpretation
nor acceptable arbitrary modulation behavior. The19873.23Hz +7 request is reported
unsupported, with no clamp or aliased output presented as acceptance.

Busy/control pre-event differences reach .023264 despite identical source
prefixes, versus at most2.992e-14 for the previous complete-bank diagnostic.
Centered source-envelope fitting/interpolation and globally estimated coefficients
are not a proven source-causal decomposition. Busy tonal-attack whole-response
q10/q50 offsets reach +4414/-5432 frames; these are reported as nonisolated music,
not relabeled as isolated attack timing. The unrelated-stereo fixture measures
left583.31Hz and general stereo deviation; it does not independently score right
947.73Hz pitch or certify interchannel coherence. All residual energies and tails
remain included, including unsupported mode assumptions and numerical error.

No estimator, envelope, source, target or gate parameter was tuned after the
first quality run. Review added only rejection of complex PCM before float
conversion, with a focused passing invalid-input/control rerun. This does not
change the measured real-PCM path. No Native, production API/ABI, installer,
App/device or listening change follows this rejected candidate. Full-range,
transient/mixture gain, source causality, aliasing, variable maps, continuous
output, realtime/resources and Human acceptance remain open.

### Source Certificates And Trailing Envelopes — 2026-10-16

The command uv run --no-sync python scripts/pitch_mode_certificate_probe.py
separates the previous energy-guard refusal from actual stationary observability.
Both frozen geometries use8192 source frames,4096 rows per channel,32 columns,
and the same rank/pole/condition/full-window-fit thresholds. Compact offsets are
j; spread offsets are floor(4095*j/31),j=0..31. X0[n,j]=x[n+offset_j] and
X1[n,j]=x[n+offset_j+1] keep the one-original-sample eigenvalue exp(i*omega).
There is no sample-rate division by the column spacing. All8192 original samples
participate in the final real cosine/sine least-squares certificate.

The first run inspects1368 certificates,18 sources times38 windows times two
geometries, in4.146 seconds. Each geometry accepts the same174 of684 windows.
Both certify the same six constant-amplitude close-pair windows beginning
40960..61440 despite the original energy ratio of .114 to .190 being below .25.
Both obtain real rank4. Compact maximum pole condition is1067.086 and full-window
relative RMS2.576e-10; spread values are1.13956 and3.371e-13. Compact retained
singular condition is1182..1239 versus1.69..1.70 for spread. This demonstrates
false exclusion by mixture-power flatness and a conditioning improvement, not a
measured inability of the compact pencil to resolve these carriers. Short attacks,
impulses, chirp and noise obtain no stationary certificates in either geometry.
Unseen-frequency controls pass within1.57e-10Hz, and exact zero/unobserved late
impulse controls refuse explicitly without NaN. Every source sample is retained.

The command uv run --no-sync python scripts/pitch_causal_modes_probe.py selects
the separate certified v2 preparation. Complete spread-window source-fit
certificates feed the existing recurrence clusters, source-only frequency
refinement and global stereo coherence checks. The original energy/crest limits
remain reported diagnostics; they are not admission gates in this candidate.
The old centered v1 path remains the default. Three source-control comparisons
load the actual40e22d7 carrier and envelope modules from Git into memory:
mono900.37, independent900/1100 gates and unrelated stereo preserve frequencies,
coefficients, complete estimator reports, centered envelope arrays and complete
envelope reports bitwise.

The v2 local envelope fitter keeps the256-frame symmetric Hann, hop64, NNLS
solver, nonnegative coefficients, conditioning and iteration limits, but uses
offsets[-255,0] and holds the last computed knot. Its fixed grid starts at the
first active source frame and continues through the finite input. It neither
appends a last-active/end knot nor masks fitted decay at a future last-active
sample. That originally planned mask was corrected by source-level reasoning
before the first v2 measurement: a later impulse can extend a source hull and
otherwise reveal formerly masked earlier decay. Every retained decay sample
belongs to the analytic modes and exact fully transformed residual; no fitted
component or resulting tail is discarded.

Fixed-basis controls use a deliberately off-grid future impulse and require
exactly equal earlier envelopes, reconstructed source modes and complementary
source errors. They also compare different last-active frames799 and1024, with
nonzero earlier fitted decay retained. Both prefix differences are zero; later
envelopes change, so the control is not vacuous. Shared NNLS/KKT, duplicate-carrier
refusal and silence controls pass. This proves local envelope prefix invariance
against the same fixed carrier basis. All-file carrier discovery/reference
selection, fractional complex interpolation, complete output causality, gain,
general mixtures, aliasing, native resources and live quality remain separate.

The first complete v2 matrix finishes72 requests in389.055 seconds:71 rendered
profiles and the same explicit19873.23Hz/+7 Nyquist rejection. All18 identities
are within7.11e-15, exact decomposition within1.12e-16, proportional stereo error
zero, and every isolated pre-target/actual finite-end energy fraction is zero.
All35 tonal measurements pass15cent, with maximum .695105cent. Busy/control
pre-event differences are at most1.646e-13, versus .023264 for centered v1.
This bounded improvement does not certify all-file discovery or exact global
causality. Busy tonal-attack whole-response q offsets still reach+4665/-5466;
these remain nonisolated music measurements.

The candidate is rejected overall:21 timing profiles fail, comprising four
original and17 unseen cases. The original mono900.37 source fails all three
shifted profiles and independent900/1100 gates fail at -7. Among unseen sources,
427/1327 fails at -7, close521/533 at all three, independent371/1381 at -7/0,
chirp/AM/noise at all three and near-Nyquist at -7/0. No gate was changed.

| Source at r2 | k=-7 q10/q50 | k=0 q10/q50 | k=+7 q10/q50 |
| --- | --- | --- | --- |
| Mono900.37 | -177/-5 FAIL | -82/+114 FAIL | -10/+135 FAIL |
| Original331/997 plus impulse | -77/+75 | -16/+90 | -1/+16 |
| Independent900/1100 plus impulse | -326/+28 FAIL | -81/+28 | -29/+91 |
| Close521/533 plus impulse | -632/-1506 FAIL | -768/-598 FAIL | +161/+96 FAIL |

For a single carrier, constant trailing NNLS estimates a carrier-energy weighted
past mean, not the current envelope endpoint. A linear envelope has bias minus
its slope times the weighted sample age, approximately127.5 source frames for
the symmetric Hann; holding a knot adds0..63 frames. That lag tendency alone
does not explain negative q10 or pitch-dependent signs. The mono source residual
has energy9.20643; transformed residual energies are16.04575/11.86306/7.20625.
Its signed mode/residual cross energies change from -7.83923 at identity to
+1.23716/+7.84756/+7.64426. Source cancellation becomes constructive interference
under different mode/residual transforms. Mono output/source energy ratios are
.509006/.510206/.507623; no output normalization conceals the change. Short
attacks and residual-only chirp/noise retain the previously rejected bank's
limitations. A smaller source residual would not alone prove a smaller wet error.

### Independent Known-Mode Mixture Reference — 2026-10-16

The additional command uv run --no-sync python scripts/pitch_mode_reference.py
uses a new impulse-free521.17/533.43Hz source with the declared amplitudes,
phases,48k/36k gates,1733-frame start difference and145/217 ramps. It changes
none of the original fixtures. Known analytic modes enter only this reference;
the estimated v2 candidate still receives PCM alone on the same control source.
The common birth is fixed at32769 source frames (SOURCE_TARGET+1), without a
fitted phase, gain, delay or crop.

The independent output-clock formula for each mode is
A_i(r*n)*cos(h*omega_i*n+phi_i-(h/r-1)*omega_i*b), where
phi_i=source_sine_phase_i-omega_i*source_start_i-pi/2. Its whole output agrees
with the explicitly known-mode renderer with zero measured PCM difference.
At r1/h1 and r2/h2 it matches canonical dry quantiles exactly; r2/h2 PCM is
exactly equal to source[::2]. All five reference tonal measurements pass15cent,
with maximum .676860cent.

Even these exact independently transported modes fail the unchanged mixture
energy timing limit at all three shifted profiles:

| k at r2 | Exact-reference q10/q50 | Estimated-v2 q10/q50 on the same impulse-free source |
| --- | --- | --- |
| -7 | -656/-1570 | -632/-1506 |
| 0 | -812/-649 | -768/-599 |
| +7 | 0/-604 | +162/+94 |

The difference-frequency energy term now beats at8.18257/12.26/18.36924Hz,
versus canonical dry24.52Hz. During the common plateau its left-channel
coefficient .0273 is about89.5% of the left-channel mean individual energy .0305;
pooled stereo scales both by1.25. Individual source envelopes
can therefore follow r exactly while the summed PCM energy quantiles move.
The phase gauge matters: the common birth analytically extends the second
carrier through its earlier silent gate, and is one declared interpretation,
not a uniquely observable physical phase before that mode begins.

This measurement establishes a contribution from the declared independent-mode
interference model. It neither excuses estimator error nor proves universal
impossibility or acceptable musical timing. Estimated versus exact-reference
whole PCM differs by up to .342941/.436350/.349693; additional envelope/residual
errors remain. Original21 timing failures, unsupported domain, gain/transient,
AM interpretation, unrelated-right-channel/coherence, aliasing, full-range,
continuous-output, resources and Human gates stay open. No productive backend,
Native/API/ABI, install, App/device or listening change follows this experiment.

### Causal Affine Endpoint Diagnostic — 2026-10-16

The command uv run --no-sync python scripts/pitch_affine_modes_probe.py selects
an additional v3 source-only envelope experiment. The v2 certified carriers,
256-frame trailing symmetric Hann, offsets[-255,0], hop64, first-active fixed
grid through finite input, left hold and complete residual renderer stay fixed.
Each carrier has two nonnegative amplitudes L/R with lambda=(offset+255)/255.
The joint weighted design contains interleaved (1-lambda)*f_k,lambda*f_k columns.
The same unpenalized bounded NNLS solver checks this actual2K-column design
against condition1e6 and128 active-set solves. Refused knots contribute zero;
their whole error, all fitted decay and all transformed tails remain retained.
Only R is published at the knot. Both Hann endpoint weights are zero, so this
is a prediction from past observations, not a directly observed current attack.

The report bounds detailed evidence to the first16 knots and counts omissions.
It retains actual-design condition, endpoint/slope summaries and the row norms
of the unconstrained weighted pseudoinverse for R. These norms are not NNLS
active-set sensitivity. The affine memory guard remains256MiB. No output gain,
phase, delay, crop, quality threshold or original fixture was fitted or changed.
Actual a26037b envelope code loaded into memory gives bitwise equal arrays and
whole reports for both old v1/v2 paths on three different source controls.

Independent physical controls precede the quality run. All58 full-window affine
knots verify current-R error at most5.00e-16. The12 full-window retained knot
details additionally verify L error5.56e-16 and slope error2.37e-18. The predicted
left-hold error agrees within5.19e-16 and still reaches .00189 between knots;
the old constant fit's effective past age is127.4985..127.5001 source frames.
A circular-stereo carrier gives an independently calculated2x2 Hann Gram matrix,
whose two-column design has condition2.766159487 and unconstrained R row norm
.368391060512. The NNLS
boundary control instead has active-set row norm .235568892433 and the known
optimum L=0,R=.223117008926, matched within2.78e-17. The unconstrained circular-case
endpoint kernel has negative lobes down to -.01114205671, sum error3.34e-16 and first-moment
error7.29e-14. Thus extrapolation removes resolved affine bias at a knot while
increasing sensitivity; it supplies no general transient quality guarantee.
Constant, silence, duplicate/near-duplicate conditioning refusal and fixed-basis
off-grid future-impulse controls pass. Same/different-hull earlier envelopes,
modes and exact complementary errors remain bitwise equal, with nonzero later
changes. Global all-file carrier discovery remains outside that causal claim.

The first complete matrix finishes72 requests in403.686 seconds:71 full PCM
outputs and the same explicit19873.23Hz/+7 Nyquist rejection. All18 identities
are within7.44e-15, decomposition within1.12e-16, proportional R=.5L error zero,
and isolated pre-target/finite-end energy fractions zero with peaks after the
event. All35 tonal measurements pass15cent, maximum .702022cent. Busy/control
pre-event differences are at most1.913e-13; busy tonal whole-response offsets
still reach+4671/-5470 and are not isolated-attack acceptance.

The overall candidate is rejected with21 timing failures, four original and17
unseen. The count equals v2, but the failing profiles differ. The unchanged
impulse, short900/percussion and nearby-impulse cases pass their measured timing
predicates; the short900 raw peak still reaches1.88481. Every failed timing
profile is retained below, including new regressions:

| Source at r2 | k=-7 q10/q50 | k=0 q10/q50 | k=+7 q10/q50 |
| --- | --- | --- | --- |
| Mono900.37 | -98/-10 FAIL | -53/+48 | +5/+70 |
| Original331/997 plus impulse | -121/-19 FAIL | -30/+39 | -16/-6 |
| Independent900/1100 plus impulse | -335/-100 FAIL | -39/+32 | -162/-53 FAIL |
| Unseen427/1327 | -173/-55 FAIL | -130/-59 FAIL | -73/-2 |
| Close521/533 plus impulse | -664/-1564 FAIL | -769/-613 FAIL | -542/-584 FAIL |
| Independent371/1381 | -84/+11 | -128/-60 FAIL | -26/-6 |
| Chirp | +224/+431 FAIL | +145/+269 FAIL | -106/-231 FAIL |
| AM interpretation | +4427/+11911 FAIL | +3557/+11898 FAIL | +333/+1334 FAIL |
| Noise | +212/+222 FAIL | +135/+102 FAIL | -222/-236 FAIL |
| Near Nyquist | -109/+21 FAIL | -77/-33 | unsupported |
| Unrelated stereo | -85/+17 | -150/-66 FAIL | -36/-19 |

The mono source remainder energy falls from v2's9.20643 to6.08008, but its
transformed remainder still has energy9.02059/6.60520/3.79453 and signed mode/rest
cross energy -.98694/+2.50917/+1.80830, versus -8.22151 at identity. A smaller
source error remains insufficient to certify the separately transformed mixture.
Close-pair maximum actual-design condition is68.20845 and unconstrained endpoint
row norm5.58096; the three-sideband AM interpretation reaches1619.41452 and
123.38499 respectively, with no tile refused by the unchanged1e6 condition limit.
Its raw peak reaches6.85979. These weighted-target row norms are not actual PCM
noise gains or NNLS active-set sensitivity; no output normalization hides failure.
Residual-only chirp/noise paths are unchanged by an envelope model with no modes.

The same run also evaluates the unchanged impulse-free known-pair reference on
five control profiles. Exact-reference/known-mode whole PCM difference stays
zero, h=r identities pass, and reference tonal checks retain their earlier
results. At r2/-7,0,+7 the exact-reference q10/q50 remain -656/-1570,
-812/-649,0/-604. Estimated v3 instead gives -664/-1564,-769/-614,-540/-591,
with whole PCM errors .243459/.304785/.205758. These differ from the nearby table
because this control source has no impulse. Individual envelope accuracy does
not remove the reference's mixture beating or excuse additional estimator error.
No universal impossibility, backend/B5, native, realtime, full-range, resource or
Human acceptance follows. The original v1/v2 diagnostics remain selectable.

### Projected Affine Prediction Diagnostic — 2026-10-16

The command uv run --no-sync python scripts/pitch_predictive_modes_probe.py
selects a separate v4 experiment. Its only signal-processing change from v3 is
publication between fixed source knots: max(0,R+d*(R-L)/255),d=0..63. Carrier
admission, the joint trailing256Hann endpoint fit, condition1e6,128-solve cap,
first-active64-frame grid through finite input and complete residual renderer
remain unchanged. Refused endpoints stay zero. Every fitted decay, projection
error, complementary source sample and transformed output tail is retained.
There is no future endpoint, retrospective interpolation or last-active mask.
Old v1/v2/v3 remain selectable; three sources times three policies compare their
complete prepared arrays and whole reports bitwise to actual e731a36 Git modules
loaded into memory, without additional source/runtime copies.

Independent physical controls cover two distinct carrier/stereo bases. Each
checks26 complete fully supported hops, source frames385..2048, including final
age63 without an appended end knot. Whole-hop envelope errors are6.11e-16 and
4.45e-16. All regular knot values are bitwise equal to v3 left hold. The12 retained
full-window details per basis additionally check endpoint/slope errors within5.56e-16 and
2.37e-18. A decreasing affine envelope crosses zero at408.5: the checked hop
385..448 precedes any later fitting window contaminated by source clipping;
projected amplitude error is7.16e-18 and negative predictions become exact zero.
This proves that declared projection locally, not exact recovery after an
arbitrary source envelope changes shape.

For a circular-stereo carrier the independent2x2 Hann Gram predicts the
unconstrained weighted-target row norm sqrt(h*inverse(G)*transpose(h)), with
h=(-d/255,1+d/255). It increases from .368391060512 at age0 to .532569384896 at
age63; measured report errors are within1.67e-16. These norms do not measure
actual PCM-noise gain or constrained NNLS/projected active-set sensitivity.
Fixed-basis off-grid impulse controls retain exact earlier envelopes, modes and
subtraction complements for both same and different last-active hulls, with
nonzero later changes and earlier fitted decay retained. Duplicate/near-duplicate
designs refuse all14 knots, leaving original PCM entirely in the complement;
silence stays exact zero. Source reconstruction error is at most2.78e-17.
Global discovery, arbitrary modulation, mixture beating and residual-only
sources remain outside these independent affine controls.

The first complete matrix finishes72 requests in416.881 seconds:71 full PCM
outputs and the same explicit19873.23Hz/+7 Nyquist refusal. All18 identities
are within7.22e-15, decomposition within1.12e-16, proportional R=.5L error zero,
and isolated pre-target/finite-end fractions zero with peaks after the event.
All35 tonal checks pass15cent, maximum .689759cent. Busy/control prefix differences
are at most2.253e-13; busy tonal whole-response offsets still reach+4672/-5470
and remain nonisolated measurements.

The overall candidate is rejected:22 timing failures, four original and18 unseen.
The long mono source now passes all three timing profiles, but independent900/1100
now fails all three, and unrelated stereo also fails at -7. All failed profiles
and the relevant mono comparison remain visible:

| Source at r2 | k=-7 q10/q50 | k=0 q10/q50 | k=+7 q10/q50 |
| --- | --- | --- | --- |
| Mono900.37 | -94/-8 | -86/+24 | -9/+57 |
| Original331/997 plus impulse | -166/-31 FAIL | -50/+25 | -61/-27 |
| Independent900/1100 plus impulse | -264/-6 FAIL | -241/-24 FAIL | -175/-64 FAIL |
| Unseen427/1327 | -243/-74 FAIL | -179/-97 FAIL | -86/-18 |
| Close521/533 plus impulse | -710/-1582 FAIL | -778/-619 FAIL | -761/-738 FAIL |
| Independent371/1381 | -82/+8 | -165/-88 FAIL | -58/-27 |
| Chirp | +224/+431 FAIL | +145/+269 FAIL | -106/-231 FAIL |
| AM interpretation | +6341/+11915 FAIL | +3811/+11900 FAIL | +386/+1606 FAIL |
| Noise | +212/+222 FAIL | +135/+102 FAIL | -222/-236 FAIL |
| Near Nyquist | -116/+10 FAIL | -79/-35 | unsupported |
| Unrelated stereo | -118/-18 FAIL | -104/-44 FAIL | -49/-27 |

The complete residual-only chirp/noise paths retain their earlier exact results.
The mono source remainder energy is6.23722; projection affects189 envelope sample
values. Close521/533 has1391 projected values and maximum age63 weighted-target
row norm7.52334. The three-sideband AM interpretation has440 projected values,
the unchanged actual-design condition1619.41452, maximum age63 row norm150.49246,
and raw output peak8.04141. These diagnostic facts retain the amplification and
conditioning limitations; no gain normalization or quality gate hides them.

The five impulse-free known-pair controls preserve zero whole-PCM error between
the independent analytic reference and known-mode renderer, and h=r identities.
Exact reference q10/q50 remain -656/-1570,-812/-649,0/-604 at r2/-7,0,+7.
Estimated v4 gives -710/-1581,-778/-619,-760/-742 on this same impulse-free source,
with whole PCM errors .235006/.285580/.269450. Reference beating and additional
estimator error remain distinct; exact affine full-hop controls certify neither.

One final review found undercounted temporary arrays in the predictor's memory
estimate. After this first run terminated, its additional allowance was increased
from24 to40 bytes per source frame, retaining the256MiB guard. This accounting-only
correction leaves prepared PCM and every other report bitwise equal on all18
matrix sources; the estimate increases by16*N,2,584,608 bytes for these inputs.
Independent prediction controls pass again. A2,800,000-frame silent stereo input
with two carriers was admitted by the old258,997,392-byte estimate and is rejected
before output/fitting allocation by the corrected303,797,392-byte estimate.
First-run raw metadata keeps its original smaller estimate; its actual fixture
inputs fit both bounds. No signal-processing change or second quality run follows
this guard repair. Product/B5/full-range/realtime/resource/Human gates remain open.

### Positive Current-Weight Diagnostic — 2026-10-16

The command uv run --no-sync python scripts/pitch_current_weight_probe.py selects
a separate v5 experiment. Its only signal-processing change from v4 is the fixed
rising half-cosine w[m]=.5*(1-cos(pi*m/255)),m=0..255, with past-start weight0
and current weight1. The same source-certified carriers, actual2K joint endpoint
NNLS, condition1e6,128-solve cap, first-active64 grid, projected next-hop formula
and complete residual renderer remain. Every fitted decay, projection error and
transformed tail is retained. No last-active mask or extra end knot is added.
Positive current weight is an observation weight, not exact knowledge of the
current envelope or a guarantee of attack quality. The four old variants remain
selectable; three sources times four policies preserve whole reports and all
prepared source/mode/residual arrays bitwise against actual1fc4ab9 Git modules
loaded into memory. No source/runtime copy is needed.

The independent current-cue control uses a .24 plateau and circular complex
stereo vector[1,-i]/sqrt(2), whose real feature energy is .5. Only source frame385
receives a predeclared signed .001 times that real feature. Direct moments derive
G=.5*sum(w*q*transpose(q)) and deltaBeta=.5*epsilon*w_last*inverse(G)*e_R,
with q=(1-lambda,lambda). For positive epsilon the endpoint changes are
[-.000030553161968,+.000023958501968]. Both signed endpoint errors are within
2.23e-16, and every predicted frame385..448 matches independent h*deltaBeta
within6.25e-17. Actual endpoints and predictions remain above .23996, proving
that NNLS and output projection constraints are inactive for this comparison.
The old Hann current-frame cue changes no sample of that immediate published
hop, bitwise; later knots may respond. This is a bounded physical response proof.

Weighted-target and original-PCM sensitivity now have separate reports. The
fixed-basis unconstrained PCM map is A^+*diag(sqrt(w)), with stereo sample weights
repeated in actual tile order. Independent G2=.5*sum(w^2*q*transpose(q)) gives
PCM norm sqrt(h*inverse(G)*G2*inverse(G)*transpose(h)), versus weighted-target
norm sqrt(h*inverse(G)*transpose(h)). At ages0/63 the respective PCM values are
.202569852344/.305565167602 and weighted-target values .218899529317/.352497276020.
Report errors are within4.45e-16. Moment-derived G agrees with the direct design
within7.11e-15; equivalent kernels reproduce constants and first past moments
within3.34e-16 and2.71e-13. These norms describe conditional unconstrained linear
maps, not sensitivity after NNLS or projection changes the active constraints.

Two independently generated carrier/stereo bases each check26 complete supported
hops,385..2048, with envelope errors at most1.61e-15. The12 retained full-window
details per basis verify endpoints/slopes within6.11e-16/2.59e-18. The projected
408.5 crossing agrees within8.16e-18 on its pre-next-fit hop only. Same/different-
hull off-grid future-cue prefixes have bitwise equal envelopes, modes and exact
subtraction complements, retain decay and change nontrivially after the cue.
Duplicate/near-duplicate designs refuse all14 knots; silence and zero-carrier
controls retain exact zero modes and the complete original source remainder.
Source closure error is at most2.78e-17. All-file discovery, curvature, general
mixtures, beating, aliasing, residual-only quality and Human acceptance stay open.

The first complete matrix finishes72 requests in422.650 seconds:71 full PCM
outputs and the same explicit19873.23Hz/+7 Nyquist refusal. All18 identities are
within8.44e-15, decomposition within1.12e-16, proportional R=.5L error zero, and
isolated pre-target/finite-end fractions zero with peaks after the event. All35
tonal checks pass15cent, maximum .685237cent. Busy/control prefix differences are
at most3.399e-13; busy tonal whole-response offsets still reach+4667/-5462 and
are not isolated-attack acceptance.

Timing failures decrease from v4's22 to13, comprising one original and12 unseen
profiles. The long mono and original331/997 sources pass all three timing
profiles; independent900/1100 still fails at -7. Unseen427/1327,371/1381,
near-Nyquist -7/0 and unrelated stereo now pass their measured timing predicates.
The latter's tonal check still measures only the declared left-channel carrier;
this grants no general unrelated-channel/coherence acceptance. Every remaining
timing failure is retained below:

| Source at r2 | k=-7 q10/q50 | k=0 q10/q50 | k=+7 q10/q50 |
| --- | --- | --- | --- |
| Independent900/1100 plus impulse | -126/-8 FAIL | -16/+27 | -32/+37 |
| Close521/533 plus impulse | -666/-1573 FAIL | -843/-658 FAIL | -117/-560 FAIL |
| Chirp | +224/+431 FAIL | +145/+269 FAIL | -106/-231 FAIL |
| AM interpretation | +3246/+11875 FAIL | +1465/+6385 FAIL | +243/+235 FAIL |
| Noise | +212/+222 FAIL | +135/+102 FAIL | -222/-236 FAIL |

The residual-only chirp/noise outputs retain their earlier exact results; short
attacks also retain the previously rejected bank's gain/transient limitations.
The mono source remainder energy is3.01516 with233 projected envelope values.
Close521/533 has remainder energy1.66783,1493 projected values, maximum design
condition78.81531 and age63 weighted-target/PCM row norms6.12275/4.94587.
The AM interpretation has remainder energy1.00898,439 projected values, maximum
design condition1622.94494, age63 norms124.11547/90.42296, and raw output peak
3.89125. Smaller source error and conditional linear sensitivity do not certify
the separate nonlinear mode/residual transforms. No normalization hides failure.

Five impulse-free known-pair controls preserve zero whole-PCM error between the
independent analytic reference and known-mode renderer and pass h=r identities.
Exact-reference q10/q50 remain -656/-1570,-812/-649,0/-604 at r2/-7,0,+7.
Estimated v5 gives -666/-1572,-842/-659,-116/-570 on that same impulse-free source,
with whole PCM errors .163358/.216447/.117831. Physical reference beating and
additional estimator error remain separately visible. The candidate is rejected
overall; no B5/backend, full-range, realtime, resource, device or Human acceptance
follows. All previous negative results and original acoustic gates remain.

### Source-Certified Positive-AM Families — 2026-10-16

The command uv run --no-sync python scripts/pitch_am_family_probe.py selects
the separate v6 offline experiment. A source-certified symmetric triple may be
represented by one central phase owner with a positive real source-clock
envelope. This changes which phase follows pitch; it does not change the v5
256-frame current-weight affine NNLS, fixed64 prediction, all-mode reconstruction
or complete transformed subtraction residual. The five prior policies remain
selectable. Three source equations times those five policies preserve all
prepared arrays bitwise and complete preparation reports exactly against the
actual55ef554 Git module compiled in memory, without source/runtime copies.

The certificate receives only original stereo PCM and source-estimated carriers.
It considers at most20 triples among the existing maximum6 carriers. Each trial
holds the central frequency fixed and refits all carriers jointly, constraining
the selected sidebands to fc plus/minus nu. Every complete8192-frame window on
the unchanged4096 raster that supports the original source fit must also support
the constrained factorization. At least3 such windows are required, with the
unchanged source RMS1e-4, design condition1e6 and coefficient coherence1e-3 limits.
The sideband vectors must share the center's complex stereo direction and have
conjugate sideband/center ratios u and conjugate(u), with 1-2*abs(u)>0. Global
source phase and modulation ratio must remain coherent across all support.
The largest actual center coefficient supplies its normalized stereo direction,
with the earliest-window tie. Overlapping certified triples are all preserved
as independent carriers; there is no output-fitted or greedy choice.

Three independent complex-stereo positive sources verify the full-PCM algebra
and center direction within9.64e-14. Five spacing, phase, channel-ratio,
coherence and positivity negatives preserve the original carriers bitwise.
Two disjoint families are admitted; two overlapping positive families sharing
a sideband are both refused. Three piecewise sources change later modulator
phase, modulation depth or common center/stereo phase. Each plateau individually
certifies positive AM and independently preserves global source coordinates
within2.54e-13. The complete sources have5 good windows followed by5 contradictory
clean windows; every original carrier is retained despite the earlier support.
Silence, empty/insufficient support and7 malformed
frequency/vector contracts are covered. These controls supply their frequencies
only to construct and independently check algebra; the full matrix instead uses
the actual source-only carrier estimator. No algebraic test establishes unique
physical AM intent for arbitrary musical PCM.

The original AM source is estimated as689.72/713.19/736.66Hz, then certified to
one713.19Hz phase owner by9 complete source windows. Its minimum positive margin
is.6 and constrained source RMS is at most4.40e-13. The unchanged envelope
fitter's maximum actual design condition becomes2.91603 versus the three-mode
v5 value1622.94494. Its source residual energy1.29752/fraction.00204881 remains
fully transformed; smaller conditioning is not itself a quality result.
Original carrier*h Nyquist refusals remain conservatively in force, and known
grouped sidebands fc*h+nu*r reaching Nyquist are additionally rejected before
processing. Independent controls exercise both guards and the required opt-in
preparation-policy chain. There is no clipping or silently widened domain.

The declared-source reference constructs the same finite AM PCM within3.24e-14,
and the sum of its three exact complex sidebands within6.37e-14. At r2 and
k=-7/0/+7 the exact grouped reference has q10/q50 offsets -3/-4,+1/-1,0/0,
whereas pitching the independent sidebands yields +338/-111,-84/-75,+216/-20.
The grouped reference retains modulation at nu*r; independent sidebands move it
to nu*h. Both h=r identities pass and grouped-reference tonal checks pass15cent.
These declared-source results neither supply oracle parameters to preparation
nor certify the source-only candidate or unique physical mixture semantics.

The first full v6 matrix completes72 requests in455.948 seconds:71 complete PCM
outputs and the same explicit19873.23Hz/+7 Nyquist refusal. All18 identities are
within8.44e-15, decomposition within1.12e-16, proportional-stereo error zero,
and isolated pre-target/finite-end energy fractions zero with peaks after the
event. All35 existing tonal checks pass15cent, maximum.685237cent. Busy/control
prefix differences remain at most3.399e-13, without isolated-attack acceptance.
Three supplemental candidate-AM tonal controls pass15cent with maximum.004667cent.
The declared source center is used only for independent spectral measurement,
never for preparation; original fixture frequency tuples/scores are unchanged.

The original AM case improves from three timing failures to two. At r2 and
k=-7/0/+7 its candidate q10/q50 offsets are -179/-78,-129/-26,-18/+1; only+7
passes the unchanged96-frame/2ms timing limit. Raw peaks are.305145/.275970/
.232702 instead of the previous maximum3.89125. Complete transformed residual
energies1.70345/1.10184/.719481 remain in those measurements. The exact grouped
reference passes while the fitted candidate still fails two requests: source
factorization and better conditioning do not prove envelope/residual quality.
No gain, output alignment, residual trimming or threshold change hides the gap.

All other previous matrix failures remain: independent900/1100 at-7 has q10/q50
-126/-8; close521/533 has -666/-1573,-843/-658,-117/-560; chirp has+224/+431,
+145/+269,-106/-231; noise has+212/+222,+135/+102,-222/-236. The candidate is
therefore still rejected overall, with12 timing failures (one original,11 unseen),
zero existing tonal failures and one explicit unsupported request. Five additional
impulse-free known-pair comparisons preserve the exact-reference/estimator
distinction and previous failures. There is no productive Native change, selected
backend, B5/full-range/variable-map/realtime/resource/device or Human acceptance.

An actionable next source-model experiment is to use the already certified
positive modulator as part of the fixed fitting basis and estimate only its
common amplitude/gate, rather than approximating that known modulation again
with affine envelope samples. This follows the source factorization, not the
output quantile targets; it has not been implemented or accepted here. A separate
bounded chirplet phase-law route also needs its own source certificate and
negative controls before replacing any residual-only path. All original gates
and rejected earlier experiments remain required evidence.

### Fixed Positive-AM Fitting Basis — 2026-10-16

The command uv run --no-sync python scripts/pitch_am_basis_probe.py selects the
separate v7 experiment. A selected source certificate supplies its global
modulation frequency nu and complex ratio u. The positive source function
m(s)=1+2*real(u*exp(2j*pi*nu*s/Fs)) multiplies the existing real carrier features
inside every source fit. The unchanged current-weight affine NNLS estimates
common amplitude before modulation, rather than approximating the already known
AM curvature again. Publication is max(0,R+d*(R-L)/255)*m(s), at that same source
frame. Analytic synthesis multiplies this physical envelope by the unmodulated
complex carrier exactly once; every subtraction error and decay remains in the
fully transformed residual. An uncertified carrier uses the stationary basis.

The model uses the unchanged source-only family certificate and global phase,
trailing256 window,64-frame grid, half-cosine observation weights, condition1e6
and128-solve cap. No output intent, fixture identifier, expected frequency or
quantile target selects the model. Sources without a certified family retain
the v6 signal path. Actual3b0dd50 envelope and mode modules compiled together in
memory verify
18 old-policy comparisons across three sources, including a positive AM source:
all v1..v6 prepared arrays are bitwise equal and whole reports exactly equal.
Two no-family v7/v6 comparisons also preserve whole reports except model version.
No source, environment or runtime copies are made.

Endpoint, slope and original prediction-row reports explicitly describe the
common amplitude. Additional physical-envelope weighted-target and original-PCM
row norms multiply the corresponding common-amplitude norm by m(c+d), separately
at ages0 and63. For circular stereo C=[1,-i]/sqrt(2), independent moments give
G=.5*sum(w*m^2*q*q.T), G2=.5*sum(w^2*m^2*q*q.T), q=[1-lambda,lambda]. Common PCM
norm is sqrt(h*inverse(G)*G2*inverse(G)*h.T), h=[-d/255,1+d/255]; the physical norm
is m(c+d) times that value. Physical interpolation is not affine even when the
common amplitude is affine. These are conditional fixed-basis unconstrained
maps, without an active-set, discovery-causality or general mixture claim.

Two independently generated nonconstant-AM stereo bases reproduce26 whole
supported affine common-amplitude hops each, with physical-envelope error at
most2.34e-15, common endpoints1.57e-15 and gate slopes9.65e-18 per frame. The
independent circular-source Gram matches direct moments within8.89e-15 and its
actual condition within4.45e-15. Common and physical row norms agree within
2.78e-16. The physical kernel obeys sum(K*m)=m_future and
sum(K*m*past_offset)=m_future*d; raw unmodulated moment identities are not assumed.

A signed physical current-frame cue of .001 times the unmodulated carrier at
frame385 changes common endpoints by [-.000029572498,.000027202770] for its positive
sign. All64 published physical response samples match independent moment algebra
within5.61e-17, endpoints within3.06e-16. A separately derived unit original-PCM
noise direction tests the age63 physical norm.241276581; signed whole-hop errors
are within7.68e-17. Both controls prove positive common endpoints/predictions for
both signs, so their checked NNLS/projection constraints remain inactive.

The public source-certificate-to-basis control mixes a lower stationary carrier
with one positive AM family. Four original carriers become two retained owners;
the flat modulator stays at index0 and the recovered global family at index1.
Four complete8192/4096 source windows support the certificate. Independently
constructed stationary expansion agrees within7.54e-14; all complete supported
prediction hops385..24512 agree with physical truth within3.25e-14, with source
closure zero. This tests certificate phase, retained ordering and mixed features.

Fixed-carrier/modulator future-cue pairs preserve bitwise-equal physical-envelope,
mode and exact-complement prefixes, with same and different last-active hulls.
Later effects are nonzero and fitted post-activity decay remains; closure is at
most2.78e-17. Projection, duplicate/near-duplicate conditioning, silence, empty
carriers and8 invalid-modulator refusals remain explicit. Additional blocked
modulation and physical-norm workspace is included before output allocation;
a2,680,000-frame silent source admitted by the old267,466,496-byte estimate is
refused by the new total before fitting/output allocation. The256MiB guard is
unchanged. These checks do not establish transformed-output quality or a Native,
realtime, full-range, resource, device or Human acceptance result.

The full unchanged matrix completes72 requests in413.505 seconds:71 complete PCM
outputs and the same explicit19873.23Hz/+7 Nyquist refusal. All18 identities are
within8.44e-15, decomposition within1.12e-16, proportional stereo error zero and
isolated pre-target/end-tail energy fractions zero. All35 original tonal checks
pass15cent, maximum.685237cent; three supplemental AM-center checks pass with
maximum.001362cent. Busy/control prefix difference remains within3.399e-13,
without granting isolated-attack acceptance.

AM q10/q50 offsets at r2/-7,0,+7 are -177/-80,-96/-29,-17/+2. The zero-semitone
request now passes exactly at the unchanged96-frame/2ms boundary; +7 remains
within it and-7 still fails. Raw peaks are.292550/.270579/.232761 and the complete
transformed residual energies1.66595/.993065/.579637 remain in every score.
Source residual energy is1.09262 and maximum actual fit condition3.18024; these
source-fit improvements are not a general quality guarantee. Every other v6
failure is unchanged, so v7 remains rejected overall with11 timing failures
(one original,10 unseen), zero tonal failures and one unsupported request.
Five known-AM and five impulse-free known-pair references retain their separate
meaning; no exact-reference success substitutes for fitted output quality.

After that run became terminal, two conditional report strings were clarified
to name common-amplitude prediction before positive modulation. The first raw
source-preparation records retain their older general interpolation wording and
their already explicit physical-envelope quantity field. This report-only
correction changes no PCM, source model, scores, limits or prior-variant reports;
focused controls are repeated, with the unchanged full matrix reused.

Instantaneous one-family amplitude projection is not a safe shortcut based on
the current certificate alone. Although E=max(0,v.T*y/(v.T*v)) is exact for a
perfect known carrier v, its original-PCM gain is1/norm(v). A scalar condition
number1 does not bound that gain near collinear carrier zeros, and small source
phase uncertainty can cause severe amplification. A further source-only gate
change-point/phase model needs bounded error and ambiguity controls; neither a
new hand-tuned denominator threshold nor output-fitted gate corrections have
been implemented or accepted here. All original product gates remain open.

### Source-Certified Integrated Linear Chirplets — 2026-10-16

The separate opt-in v8 diagnostic only tries a single isolated linear chirplet
after v7 stationary carrier discovery returns K=0. Sign-changing source zero
crossings initialize a local quadratic phase. Complete original stereo windows
then undergo bounded projected variable-projection refinement. The unchanged
8192/4096 scan, minimum three clean supports, source RMS1e-4, design condition1e6
and normalized complex stereo coherence1e-3 remain necessary. Source energy and
crest guards apply. Multiple supported incompatible frequency laws refuse.

An immutable law at source epoch e stores frequency f_e and slope alpha. With
tau=(s-e)/Fs, integrated cycles are f_e*tau+alpha*tau^2/2. All support fits share
that gauge; complex stereo coefficients are compared without independently
aligning window phases. A scan beyond4096 complete windows refuses conservatively.
Matrices remain window sized; compact scan reports and temporary work have a
reported32MiB estimate under a64MiB estimator budget, excluding caller PCM storage.
Neither zero crossings nor a successful source fit establish general musical
mode semantics.

The existing positive current-weight256/64 affine NNLS evaluates this integrated
phase in its carrier features. Its solver, rejection limits, interpolation of
amplitude predictions and full decay remain unchanged. Additional source-length
phase temporaries and bounded tile workspace enter the existing256MiB fitting
guard before output allocation. Analytic synthesis uses the same epoch law.
For constant source rate r and pitch ratio h the correction is
exp(i*(h/r-1)*(theta(s)-theta(b))), at unchanged fixed birth b. Continuous phase
algebra gives instantaneous output frequency h*f(r*n), with slope h*r*alpha;
using instantaneous frequency times elapsed time would integrate the slope twice.

Original active source support must satisfy the source frequency band. Rendering
separately guards the entire nonzero fitted analytic mode and its +/-1-frame
zero-extended interpolation support against DC/source Nyquist/output Nyquist.
Even an infinitesimal final nonzero sample remains part of that guard. No end
mask or crop hides a crossing. Exact subtraction error remains in the fully
transformed residual, including every generated tail. Fractional interpolation
is explicitly an approximation to continuous analytic phase, while h=r retains
the complete combined-source linear interpolation identity.

Independent ascending/descending unseen sources each supply five clean windows.
Full frequency-law and slope errors are below4.55e-13Hz and4.55e-13Hz/s; common
epoch stereo factorization relative RMS is below4.92e-13. A separate public
prepare-to-render descending source at r2/k+7 measures actual mixed-output
frequency/slope errors3.19e-12Hz/1.37e-12Hz/s on10752 supported interior frames,
with PCM error2.74e-13. These declared synthetic controls do not certify general
residual or finite-onset quality.

Two fixed known phase laws reproduce full positive affine prediction hops385
through2048 with maximum envelope error6.03e-15. Independently constructed joint
stereo Gram/cue responses agree within3.06e-16 for both cue signs across whole
64-frame hops. Fixed-phase future cues, including changed activity hulls, preserve
envelope/mode/complement prefixes bitwise, retain decay and cause a nonzero later
response. This is local fixed-basis causality, without a global discovery claim.
Known integer-position mode corrections agree within1.55e-14; a nonzero residual
retains its complete transformed tail with energy.00364556 beyond nominal length.
Full h=r fractional identities agree within1.12e-15. A separate whole finite
response comparison at r=.75/k+7 differs from the declared continuous ideal by
.0643066, including finite edges; it is expressly not analytic interpolation
exactness or an acoustic PASS.

Nine independent source-negative categories refuse, including unrelated stereo,
phase jumps and two incompatible laws each with exactly three clean supports.
Three complete-support band controls include an interpolation-only crossing and
an infinitesimal1e-30 last sample. Seven malformed/workspace contracts refuse.
The same2.4-million-frame two-carrier silent input is accepted by the prior
public fitter with estimated263179376bytes, but additional phase workspace
refuses before fitted output allocation. Twenty-one actual-HEAD dual-module
comparisons preserve every prior preparation array and whole report; a shifted
v7 AM render is also bitwise/report identical. V8 fallback changes only its
explicit model and source-refusal metadata.

The unchanged18-source/72-request matrix completes in429.416076s:71 complete PCM
responses and the same explicit19873.23Hz/+7 Nyquist refusal. The chirp now admits
one law at source epoch56767.5, frequency849.990625Hz and slope900Hz/s, supported
by nine clean complete windows. Maximum common-gauge coefficient distance is
7.90e-14, source support relative RMS5.06e-13, actual envelope condition2.94859
and reported fitting memory20735840bytes. Residual source energy1.07763645 remains
fully transformed; source energy864.106851 is not used to normalize the output.

At r2 and k=-7,0,+7, chirp q10/q50 offsets improve from224/431,145/269,-106/-231
to-72/-42,-22/+14,-15/+17frames. All three now pass the original96-frame/2ms
bound and the complete timing/retention/tail predicate. Raw peaks are.26438828,
.25593531,.20520025; transformed residual output energies.94288685,.83374978,
.82742422 and full output energies433.80498,433.55260,433.56577 remain unscaled.
Its r1/k0 response has zero quantile offsets. All67 non-chirp complete profile
measurement records equal v7 exactly after excluding only model version and
elapsed seconds. All35 existing tonal measurements still pass, with the same
maximum.685237cent; the full decomposition error remains below1.12e-16.

V8 is still rejected overall: eight timing failures remain, comprising900/1100
at-7, all three close-pair requests, AM at-7 and all three noise requests. AM at0
still has no margin beyond its exact96-frame boundary. Prior failures are retained
in earlier results. The unchanged additional AM tonal and known-AM/known-pair
references from v7 are reused, rather than reported as fresh v8 executions.

Source without an admitted chirplet retains the v7 signal path. Every prior
variant and explicit unsupported-domain refusal remains available. The unchanged
full source/profile matrix and original timing, tonal, peak and retention gates
still determine measured quality. Variable actual-rate maps, arbitrary mixtures,
native realtime execution and all remaining product/Human gates remain open.

### Conditional Finite Positive-AM Source Gate — 2026-10-16

The separately selected v9 experiment tries a finite raised-cosine gate only for
one already certified positive-AM family with no other retained carrier. Its
interior certified reference is explicitly restricted to the candidate plateau;
AM algebra does not itself establish that physical interpretation. All other
sources and every finite-gate refusal retain the v8 signal path. The prior locally
causal envelope policies remain unchanged. This whole-source alternative is
offline and noncausal.

With fixed original carrier vector C and positive modulator m(s), the stereo I/Q
columns are Q=[m*Re(C*exp(i*w*s)), -m*Im(C*exp(i*w*s))]. A global complex multiplier
and integer onset/rise/end/release parameters determine the modeled source.
The finite gate is zero at its onset/end and has a plateau between its ramps.
The admitted modeled onset supplies its fixed phase birth, separately from the
first observed nonzero sample. No musical target, source progression or residual
support moves. Physical synthesis uses amplitude*gate*m and one rotated unit
complex carrier direction; all unmatched PCM remains in the exact transformed
complement, including samples outside the modeled finite gate.

Before measurement the search is fixed at512-frame margins around each observed
activity edge. Onset spans first-active minus511 through first-active, end spans
last-active through last-active plus511, and each width spans3..512 intervals.
Both1025-sample neighborhoods must be inside the finite input and disjoint, with
the entire8192-frame reference inside every candidate plateau. Global and EACH
edge relative RMS must pass1e-4 independently. Joint I/Q avoids division by a
carrier sample. Plateau rank and the local six-parameter Jacobian use the existing
1e6 condition limit; condition number is not an absolute physical error bound.

Complete candidate accounting uses bounded edge sufficient statistics, direct
SVD residual checks and full original-source residual confirmation. All feasible
pairs contribute to landmark ranges and multiplier ambiguity. At most4096 edge
survivors and2000000 paired evaluations are permitted, in batches no larger than
512; incomplete search refuses, without best-K truncation. Any feasible model
touching a declared search boundary refuses. All feasible onset/rise-end/release-
start/end positions must span at most ONE source frame and complex multipliers
must agree within1e-3. This fixed source-grid resolution is separate from the
unchanged96-frame acoustic timing gate. The same256MiB allocation limit includes
128bytes per source frame plus64MiB bounded workspace before fitting allocation.

This is conditional isolated-model validity. Observed residual does not bound
unknown physical model mismatch or prove unique musical intent. Original phase,
finite-support, tonal, peak, retention and full-response criteria remain required;
no native, realtime, variable-map or Human gate is closed by these equations.

The reproducible command is
`uv run --no-sync python scripts/pitch_finite_am_gate_probe.py`. Independent
controls precede the unchanged full matrix. Two unseen32768-frame elliptical
and collinear stereo sources recover all four source landmarks exactly; the
collinear source has deliberately exact carrier zeros adjacent to both edges.
Its first/last nonzero PCM samples are two frames inside the modeled onset/end.
Global complex-multiplier relative errors are at most9.46e-14. At scales2**-400
and2**400, fresh scale-normalized whole-analytic errors are1.215e-13 and complete
complement reconstruction error is zero. The attenuated control was corrected
to divide the reconstructed signal by its actual scale before comparison;
the fitter, source, limits and search policy were unchanged.

Public PCM-only source certification, finite fitting and complete integer-rate
output are separately checked on both sources. Frequency error is at most
3.41e-13Hz, whole-source analytic error6.92e-13 and complete r2/k-7,0,+7 output
error6.73e-13. Fractional h=r combined-source identities retain a declared nonzero
complement within2.23e-15, and a nonunity complement-tail control retains energy
.00590972785. These identities do not establish fractional interpolation quality
against a continuous physical model.

Ten negative source categories refuse: a sub-domain two-frame edge, a carrier-zero
three-frame edge with only one informative interior sample, no plateau, widths
outside the frozen grid, one-sample pulse, silence, censored onset, an independent
plateau transient, an independent outside-gate tail and additional stereo carriers.
The underresolved case has both a failed Jacobian and unresolved boundary; its
primary refusal reason is the boundary, so it does not isolate the rank branch.
Twenty-two malformed contracts and two invalid reference geometries also refuse.
A read-only two-million-frame broadcast source refuses the323108864-byte estimate
against268435456bytes before source scanning or fitted-output allocation.

The predeclared20Hz carrier/2Hz modulator/eight-frame weak-edge source instead
has exactly one feasible pair and recovers its original gate and gauge. The
initial assertion that this source would be ambiguous failed; the unchanged
construction is retained as a uniquely identifiable control. An independent
demonstration of multiple feasible landmark or complex-multiplier ambiguity was
still missing from that initial evaluation, as was empirical cap-exhaustion
coverage. The added source control below demonstrates the landmark case;
complex-multiplier ambiguity and the finite-gate evaluation task remain OPEN.
The full matrix is exploratory evidence, not completion of that required sequence.

Actual prior-HEAD comparisons cover four sources and all eight old preparation
policies:32 complete reports and prepared arrays are exactly equal. Two shifted
AM/chirp renders also have exactly equal PCM and reports. Three noneligible v9
fallbacks preserve the prior prepared state except model/attempt metadata.
An eligible certified AM source with its onset censored at the input boundary
also refuses and preserves every prepared array and complete shifted PCM exactly;
reports differ only by the declared version and attempt metadata. An initial
unfiltered renderer-report assertion failed on that expected version field,
while the numerical comparison was exact; no fitter correction was needed.
The complete source-control pass is reused after adding summary-only keys; only
the two scale controls are rerun after strengthening their normalization check.

The frozen full18-by4 matrix completes in433.772387seconds:71 complete outputs
and the unchanged explicit19873.23Hz/+7 Nyquist refusal. Only the AM source admits
the finite model. Its onset/end32768/80767 and both127-frame ramps have exactly
one feasible pair out of261120 candidates per edge. Direct global/onset/release
relative RMS are1.4234e-12/2.1167e-12/2.9089e-12; the rank-six Jacobian condition
is9871.94 and the conservative allocation estimate87785728bytes. Its exact
complement retains source energy1.28318e-21. No subtraction error is discarded.

At r2 and k-7/0/+7 the AM q10/q50 offsets are now -2/-4,0/-1,0/0 frames, all
inside the original96-frame/2ms gate. This replaces -177/-80,-96/-29,-17/+2;
the previous exact96-frame boundary now has margin. Raw peaks remain about
.19599 and complete transformed complement energies9.266e-22/7.235e-22/
6.467e-22 remain in the scored outputs. All67 non-AM complete measurement
records equal v8 exactly after excluding only model version and elapsed time.
All35 existing tonal checks still pass15cent, maximum.685237cent; three fresh
additional AM-center measurements pass with maximum.001362cent. All18 identity
errors remain at most8.44e-15 and complete decomposition at most1.111e-16.

The candidate remains rejected overall with seven timing failures: independent
900/1100 at-7 has q10/q50 -126/-8; close521/533 at-7/0/+7 has -666/-1573,
-843/-658,-117/-560; noise has+212/+222,+135/+102,-222/-236. Earlier exact
AM/close-pair reference evidence is reused, not freshly measured here. Production
code, Native/API, installations, device/listening acceptance and every remaining
parent gate remain unchanged. No native build or extra Release is warranted by
this opt-in offline-only change; prior unchanged private Release/API evidence is
reused. The remaining ambiguity controls must be demonstrated before this finite
evaluation can be closed, independently of the improved AM acoustic measurements.

### Independent Multiple-Feasible Finite-AM Gates — 2026-10-16

The standalone command
`uv run --no-sync python scripts/pitch_finite_am_ambiguity_controls.py` exercises
an independent declared-source construction through the unchanged v9 fitter.
The same control now runs inside the canonical finite-AM source controls before
the quality matrix. No source-selection, search, fitting, rendering or production
policy changes. Previous complete matrix and Native/API evidence are reused,
not freshly rerun for this control-only addition. Finite-gate evaluation and
the overall acoustic candidate remain OPEN/rejected.

The source has32768frames at48kHz, carrier1000Hz, stereo direction(1,i)/sqrt(2),
global multiplier .287*exp(-.61i), and positive modulation
m(n)=1-.999998*cos(2*pi*(n-2052)/480). Independent real sine/cosine synthesis and
sine-squared gate equations generate the PCM. The true onset/rise/end/release
tuple is(2048,8,28456,8); an alternative is(2046,12,28458,12). Corresponding
onset, rise-end, release-start and end landmarks differ by TWO frames. Both
observation edges contain at least one full480-frame informative plateau period,
while the modulation trough suppresses only the short ramps.

Quadrature stereo gives the pointwise I/Q Gram F'F=m(n)^2*I/2. Consequently the
alternative complete global least-squares multiplier is z times
sum(gtrue*galt*m^2)/sum(galt^2*m^2), independently of the fitter. Before search,
this scale is1.000000000011966 and its global/onset/release relative RMS are
1.35243542e-6/7.09207263e-6/7.09207263e-6, all below the unchanged1e-4 threshold.
A separate analytical upper bound is3.411e-5: differences occupy only ten
samples per edge with m<.002144 and abs(gtrue-galt)<.11; the full plateau period
supplies scalar energy at least480 and the global multiplier correction is
less than1.24e-9. The source therefore has genuinely multiple feasible models.

A third independently evaluated tuple(2049,3,28456,8) also passes global and
each-edge RMS, with onset error3.288e-6. It touches maximum onset anchor and
minimum width. Boundary refusal therefore has priority over a displayed landmark
ambiguity refusal reason. The true model's six-parameter Jacobian also exceeds
the condition limit. Neither isolated landmark-refusal nor isolated Jacobian-
refusal branch coverage follows from this combined negative construction.

This source cannot test complex-multiplier ambiguity. Every candidate gate is
zero outside U=[1538,28966] and one on P=[2561,27943]. For two globally feasible
models, F*(beta1-beta2) on P equals the difference of their source residuals.
The global RMS limit epsilon and 0<=gate<=1 imply normalized multiplier diameter
at most 2*epsilon*K/(1-epsilon), where K^2 is the largest union-support basis-Gram
eigenvalue divided by the smallest common-plateau eigenvalue. Here the pointwise
isotropic Gram and nonnegative480-periodic m^2 bound K^2 by58/52 complete periods.
The resulting bound .0002112446663 is BELOW the existing .001 ambiguity limit.
This is a conditional mathematical exclusion for these declared source models,
not empirical coverage of the complex-ambiguity refusal branch or a general
physical-error guarantee. The independent complex-multiplier ambiguity control,
empirical cap-exhaustion coverage and full finite-gate evaluation remain OPEN.

The unchanged exhaustive search completes in303.305894seconds with261120 edge
candidates and194 survivors on EACH side. All37636 Cartesian pairs are directly
confirmed feasible and counted, with no cap exhaustion or partial selection.
Onset spans2036..2049, rise-end2048..2068, release-start28436..28456 and
end28455..28468. It publishes no gate or multiplier and reports
`feasible_set_touches_search_boundary`, exactly as independently predicted.
All global I/Q designs pass condition; the best direct fit recovers the original
gate with rank-six Jacobian condition3743820.30, above the unchanged1e6 bound.
The complete original PCM remains bitwise unchanged; the allocation estimate is
71303168bytes. Complex distance remains null because landmark disagreement already
precludes admission, as required by the existing implementation.

The first frozen-source fit is reused rather than duplicated: the final control
constructor is verified bitwise equal to that source, and its independent equations
and final report assertions are executed against the actual terminal search.
The final canonical full probe is not freshly rerun. The control addition changes
no fitter, source estimator, renderer, threshold or previously measured acoustic
profile. This evidence closes only the independent multiple-feasible landmark
negative-control item; the separately listed remaining items stay open.

### Independent Finite Carrier Gates And Sparse PCM — 2026-10-17

The separate offline `joint_finite_gates=True` experiment extends the complete
v9 policy with independently gated stationary carriers. It does not change the
Native backend, old policies, output intent, source clock or acoustic criteria.
The current PCM-only dispatch requires exactly two existing stationary carrier
certificates and reuses a source-only refinement reference. No fixture label,
known impulse position, output request or target selects a source model.

The conditional family is `y = A(g) beta + T_S d + e`, with a separate integer
raised-cosine gate per carrier, all joint real I/Q cross terms and at most one
arbitrary stereo nuisance frame. Gate widths remain 3..512. Original-PCM global
and each 1025-frame edge RMS limits remain `1e-4`. The nuisance is an explicit
part of fitting, never deleted from rendering: preparation publishes the exact
whole residual `y - A(g) beta = T_S d + e`, including every complementary sample.
Each carrier keeps its own fitted onset and global complex phase.

An admitted 8192-frame source reference is tested in sixteen 512-frame bins.
Every proper carrier subset, after every possible one-frame deletion, must have
minimum error above the global budget. Each other carrier's two ramps can meet
at most four bins, so five completely absent bins would contradict this test.
This proves a common central 2048-frame plateau for every feasible declared
model. All 2049 central support possibilities contribute coefficient ellipsoids.
The resulting conservative I/Q box permits a complete source-grid anchor scan
and bounded full ramp enumeration. A deterministic stride-16 partial-ramp lower
bound avoids unnecessary full-ramp work; omitted nonnegative errors cannot make
that necessary bound exclude a feasible model. One largest frame contribution
is subtracted across each disjoint observation union, never once per overlap.
Joint flank evaluation reuses constant-gate frame costs through 256-frame block
sums and maxima. It evaluates only the disjoint variable ramp interiors for
each tuple; prefix differences include outward cancellation allowances before
the unchanged full-flank reduction bound. Complete tuple enumeration is retained.
The canonical integrity controls include `scripts/pitch_joint_flank_bounds_controls.py`,
which compares 72 retained K1/K2 onset/release tuples with dense full-flank
equations, including block boundaries,
overlapping ramps, widths3/256/512 and the first variable release frame at the
plateau end. This regression does not claim universal floating-point coverage.

Surviving joint gate tuples examine every possible sparse support through the
same conservative SVD/downdate implementation. Direct full PCM confirms the
survivors. Global coefficient ellipsoids contribute to all reported phase ranges
and the nuisance-projected `6K` Jacobian perturbation bound. A representative's
failed edge test can exclude a tuple only when a separate edge projection proves
infeasibility; unresolved feasibility refuses. Local rank or an optimizer alone
does not prove unique geometry. Gate landmarks must agree within one frame and
source-normalized complex coefficients within `1e-3` across the entire retained
set. Boundary, ambiguity, numerical uncertainty and incomplete search refuse.

Fixed caps are 4096 surviving anchors/edges per side, 2000000 width evaluations
per side, 2000000 joint flank combinations/pairs, 4096 direct model/support
confirmations, 4096 stored joint survivors per flank and 64-width batches. The
joint survivor cap applies before appending objects, including simultaneous
onset/release storage. Final coefficient rescaling checks finite, nonzero
amplitudes before publication. The preallocation guard is 512 bytes per
input frame plus 96MiB, inside 256MiB. These are conditional resource limits;
the valid no-nuisance control currently exposes a support-confirmation cap.
That refusal remains a coverage limit and preserves the previous signal path.
Floating allowances assume the stated arithmetic/libm model; no universal
floating-point, physical interpretation, realtime, device or Human claim follows.

Reproduce source controls with `uv run --no-sync python
scripts/pitch_joint_finite_gate_controls.py`; the full ordered controls and
unchanged 18-source/four-profile measurements use
`uv run --no-sync python scripts/pitch_joint_finite_gate_probe.py`.
Existing finite-AM ambiguity/phase requirements and all full product gates remain
open independently of this experiment's measured outcomes.

The completed source controls pass seven independent positive configurations and
two amplitude scales, seven model refusals, fourteen malformed contracts, the
actual preallocation guard and both publication guards. Whole public analytic
error is at most6.91e-13; complete integer references are within3.86e-13 and
fractional identity within4.45e-15. The transformed residual retains a measured
nonunity tail energy0.0059097. Separate final controls prove bitwise unchanged
source, modes, births, phase laws and whole residual for impulse, mono, chirp and
an actual two-carrier/two-transient joint refusal. This control exercises the
lower-level joint source search. Both the dense partition check and fallback checks
are included in the canonical integrity controls; their fresh separate run took
4.0498 seconds.

The unchanged full matrix completes72 requests in605.4226 seconds, or679.2476
including source controls:71 complete PCM responses and the same explicit
19873.23Hz/+7 Nyquist refusal. All18 unity errors are at most7.67e-15 and complete
source decomposition at most1.12e-16. All35 tonal measurements pass15 cents,
with maximum1.17344 cents. Proportional stereo error, isolated pre-event energy
and actual final retirement energy are zero; every isolated peak follows its
source event. The51 PCM reports for sources not admitted by the new fitter are
exactly equal to v9 after excluding only model name and measured runtime.

Five unchanged matrix sources receive complete joint source fits:331.37/997.19Hz plus
impulse, independently gated900/1100Hz, unseen427.43/1327.89Hz, close521.17/533.43Hz
and independently gated371.39/1381.77Hz. The900/1100 timing failure is repaired,
but six full-response timing failures remain. At rate2, q10/q50 offsets are:

| Source | k=-7, frames | k=0 | k=+7 |
| --- | --- | --- | --- |
| Independently gated900/1100Hz | +47/+62 | -7/+47 | -50/-1 |
| Close521.17/533.43Hz | +31/-947 | +172/+119 | -797/-798 |
| Noise | +212/+222 | +135/+102 | -222/-236 |

The close pair's gates match the generating source exactly, with one complete
feasible model and residual energy0.171125, equal to the retained impulse. Its
remaining timing failure therefore survives correct source fitting. Cross-carrier
interference after transformation requires further work. Noise remains the
unchanged zero-mode AM/FM residual path. Running background and unrelated-stereo
inputs expose the support-confirmation cap; two busy multi-event inputs have no
feasible declared one-frame model. These refusals preserve v9 and remain coverage
limits. The entire candidate is still rejected by the full quality gate. No Native
build, device or Human listening acceptance follows from this offline delivery.

### Explicit Source-Group Amplitude Replacement — 2026-10-17

The separate offline experiment in `scripts/pitch_group_magnitude_probe.py`
evaluates one frozen amplitude-changing operator. It does not change the existing
renderer, v10 default, Native backend, public API or source fitting. Eligible
sources must already have a complete two-carrier joint fit, stationary phases
and one common complex stereo direction. A source-only projection check uses
64 machine epsilons; it does not establish physical source uniqueness. Fixture
labels and expected frequencies appear only in source generation and scoring.
There is no close-frequency cutoff, fitted output gain/phase/delay, component
weight or identity special case.

At the existing source coordinate, complex-linearly sample each complete analytic
mode first. Let Z be their sum and V the sum after the existing independent
pitch-phase corrections. The candidate is G=norm(Z)*V/norm(V), using the shared
complex stereo norm. The real grouped output replaces the original real tonal
sum while the complete transformed residual and every tail remain in place.
This is deterministic time-varying analytic amplitude replacement and can create
additional sidebands. Preserving norm(Z) is not proof of real-PCM energy timing,
tonal accuracy, alias safety or unchanged gain.

The bounded pure kernel uses stable nested hypot, a common binary normalization
and phase resolution norm(V)>64*eps*sum(norm(v_k)) at source-active frames.
Source-zero frames produce zero; positive source magnitude with undefined or
unresolved transported phase explicitly refuses. There is no epsilon denominator
or clipped gain. Numeric range loss, unrepresentable output and the existing
256MiB logical kernel working-memory limit also refuse. Its N*(64*K+384)+1048576
estimate covers kernel logical inputs, output and scratch, not process RSS,
source preparation or the baseline renderer.

An actual counterexample exposed false cancellation under arbitrary peak
normalization: .7 and nextafter(.7,+inf) both divide by2.7 to the same float,
although their difference is nonzero. The final kernel instead scales by the
highest power of two at or below the finite input peak, retaining both values
separately in its report. Every real/imag component must scale back exactly.
That round-trip check also rejects nextafter(2*TINY,0)/2 rounding up to TINY;
testing only subnormal results would miss it. Both genuine regressions refuse
without erasing the source-active frame.

Independent controls cover a scalar complex/math.hypot oracle, unchanged input
bytes, homogeneity at 2**(-400) and 2**400, complex interpolation before the norm
at four fractional-coordinate identities, unitary stereo/global-phase covariance,
orthogonal-channel kernel identity, eight malformed inputs, actual extreme
dynamic range, phase singularity, range overflow and a two-million-frame readonly
broadcast memory refusal. The orthogonal-channel check concerns the pure operator;
it does not expand the caller's common-direction source admission.

For stored residual R, stored grouped real output g and two stored transported
real components c1/c2, substitution is B+(g-(c1+c2)), where the existing baseline
forms B from R and the components. A conservative gamma6 addition bound uses
absolute component peaks and an upper bound on R inferred from B. This covers
those stored additions and the direct R+g comparison, excluding interpolation,
libm and kernel errors. Zero-tonal tails retain the residual's values; no
signed-zero bit identity or general total-error bound is claimed.

The final frozen two-source comparison is **rejected**. All eight original
q10/q50, retention, pre-event and retirement predicates pass. At rate2, complete
response q10/q50 offsets in frames are:

| Source | k=-7 | k=0 | k=+7 |
| --- | --- | --- | --- |
| Independently gated900/1100Hz | +14/+62 | -9/+64 | -5/-3 |
| Close521.17/533.43Hz | +18/+11 | -3/-1 | -1/+1 |

The original timing predicate uses q10/q50. The900/1100Hz/-7 q90 offset is -105
frames and remains reported; these measurements do not place every quantile
inside96 frames. All900/1100 tonal checks pass with maximum .03429583cent,
but substantial extra peaks appear, such as200.4398/467.1214/1000.6527Hz at-7.
The close pair passes -7 and+7 at2.019275/.204155cent, yet fails k=0 at
77.84790925cent. Its actual strongest two dense peaks are533.4281773 and
557.9640695Hz against the expected521.17 and533.43Hz. The desired521.1174806Hz
peak ranks third. No target-guided peak selection, parameter search or wider
matrix is used to hide this new failure.

Six combined h=r identities at rates.75,1.25,2 have full-PCM error at most
7.66054e-15. For both actual sources at rate2/-7, direct complete residual+G
error is8.32668e-17, below respective stored-addition bounds1.21786e-15 and
9.23319e-16. Both retain all56770 zero-tonal tail frame values. The earlier
arbitrary-normalization run is retained as negative evidence; fixing its actual
numeric bug repeats only this same frozen comparison and does not change the
candidate's rejection.

Reproduce the arithmetic controls with
`uv run --no-sync python -B scripts/pitch_group_magnitude_controls.py` and the
controls plus frozen sources/profiles with
`uv run --no-sync python -B scripts/pitch_group_magnitude_probe.py`.
These are explicitly invoked offline diagnostic scripts, not automatically
discovered pytest tests. The existing v10 result remains six timing failures
and no tonal failures across its unchanged full matrix. No baseline renderer,
admission, acoustic threshold, Native installation or Human acceptance changes.

### Residual Noise Band-Energy Attribution — 2026-10-17

`uv run --no-sync python -B scripts/pitch_noise_energy_probe.py` audits the
unchanged unseen_noise source through the existing finite-bank AM/FM operations.
It changes no source, cue, exact-zero trim, phase rule, sampler, acoustic gate or
renderer. The fixed requests are rate2 with -7,0,+7 semitones and k=12, where
h=r=2 is the unwarped control. The complete nominal silence and every finite
convolution tail remain captured.

For each band b, the audit first adds all source-segment contributions in the
global capture to form u_b. It then computes A=sum_b norm(u_b)^2, E=norm(sum_b
u_b)^2 and signed C=E-A. Squaring segment atoms first would misclassify same-band
segment cancellation as cross-band interference. Independent controls distinguish
that case from cancellation between two different bands and compare signed
smoothed balances against explicit scalar window sums. The actual frozen noise
has one active segment; these results do not claim a measured multi-segment case.

The audit uses the unchanged centered25-frame box. Only nonnegative A,E and dry
energy receive quantiles. At each fixed canonical dry quantile time t_p, it
reports R_X=sum(envelope(X) through t_p)-p*sum(envelope(X)), for X=A,E,C.
Every balance and signed integral uses the same clock's dry energy denominator.
The h=r control is subtracted separately. Signed C receives neither quantiles
nor a CDF. An explicit length/epsilon/L1 comparison budget bounds the floating
balance identity; it is separate from the unchanged2ms audio gate.

The complete43.552-second audit passes its numerical controls. Source-grid
reconstruction error is2.77556e-16; the decimated h=r output error is2.49801e-16
and its whole-band autoenergy matches decimated source-grid autoenergy exactly.
All four full audit outputs equal the corresponding frozen renderer PCM in
this one-active-segment case. Their unchanged q10/q50/q90 offsets are:

| k | Whole response, frames | Band autoenergy, frames |
| --- | --- | --- |
| -7 | +212/+222/+223 | +202/+125/+141 |
| 0 | +135/+102/+211 | +205/+123/+140 |
| +7 | -222/-236/+77 | +200/+122/+142 |
| +12, h=r control | 0/0/0 | +200/+123/+142 |

Delayed band autoenergy already exists in the unwarped control, whose whole
response reconstructs the dry signal. Its cross terms restore the original
chronology. Phase warping changes those cross terms strongly while the band
autoenergy changes much less. For example, normalized q50 balance differences
from the unwarped control are:

| k | Whole delta R_E | Auto delta R_A | Cross delta R_C |
| --- | ---: | ---: | ---: |
| -7 | -.0040671764 | -.0000225144 | -.0040446620 |
| 0 | -.0014979062 | +.0000182334 | -.0015161396 |
| +7 | +.0042641943 | +.0000284554 | +.0042357389 |

Across all three fixed boundaries and shifted profiles, absolute auto-balance
change is at most6.927e-5; the cross changes explain most of the whole changes
in this measured decomposition. This is attribution within the frozen model,
not proof of a unique physical cause or a pure per-band allpass delay. Source-grid
real autoenergy has q10/q50/q90 offsets+236/+248/+235 source frames; complex
coefficient autoenergy has+235/+247/+236. Complex magnitude alone cannot establish
the timing of the real squared output.

The normalized balance identity error is at most3.993e-15 across output profiles
and1.028e-14 on the source grid, below explicitly reported conservative budgets
up to3.321e-5. Independent arithmetic controls have scalar-window error0 and
linear-balance error2.887e-15. There is no timing tolerance change. Logical
working storage is estimated at140467917 bytes below256MiB, using sequential
bands and cached segment FFTs; no513-band PCM matrix is allocated. The initial
frame guard also refuses a real readonly two-million-frame broadcast before
cue/capture allocation. This logical estimate is not measured total process RSS.

The next constructive work must address cross-band phase relationships in
residual synthesis. Another independent-band envelope adjustment or fitted delay
would not follow from these observations. No replacement synthesizer, Native
build, realtime/resource acceptance or Human listening acceptance is delivered;
the three noise failures and the baseline's six total timing failures remain.

### Complete-Bank Joint Residual Phase — 2026-10-17

The separate offline experiment in `scripts/pitch_joint_residual_phase.py`
addresses the noise audit's cross-band observation by summing every band and
every source segment before one phase extraction. It leaves all previous
renderers, source estimators, production DSP and APIs unchanged. It uses no
fixture labels, fitted gain, component weights, phase offsets, output delay,
cutoff, window search or magnitude replacement.

For the existing1024-tap bank the mathematical complete complex kernel is
H[l]=delta[l]+i*q[l], with q[l]=2*w[l]/1024*cot(pi*l/1024) at odd l and zero
at even l. The existing cosine window has an exact final zero tap. Thus the
global source field is Q=x+i*(x*q). Four direct real convolutions, separating
source parity and channel, retain every quadrature tail and the exact structural
zeros. There is no FFT or amplitude gate. The algebraic kernel differs from the
floating computed513-band sum by at most8.339e-15 in the impulse control;
bitwise equality with that roundoff-affected sum is not claimed.

The existing pooled elapsed phase Psi is computed once on Q at reference omega0.
It keeps the principal negative-pi tie, largest-channel pooled-zero fallback,
exact-common-zero runs and birth semantics. Source cues do not reset it.
At the original source coordinate s, the frozen output is
Re(L(Q,s)*exp(i*(h/r-1)*L(Psi,s))). Complex interpolation precedes phase action.
A common rotation preserves relative band phase but does not establish real
cross-energy or timing preservation. This is distinct from replacing magnitude
while keeping the sum of independently pitched modes' phase.

Source and Q have immutable bytes backing, including protection against later
caller mutation. The nominal silence and original signed capture bounds remain:
origin=min(0,floor(-1/r)+1), nominal=ceil(Nsource/r), and
retired=max(nominal,floor((last_nonzero+1023)/r)+1). All-silent inputs retire at
nominal; empty inputs are empty. Bounds use the exact supplied float ratio as
integers to avoid preflight overflow at tiny positive rates. Invalid input,
unbounded logical allocation or observed numerical overflow refuses before
publishing output. The256MiB logical estimate includes immutable-copy overlap,
prepared arrays and render scratch; it is not measured total RSS. A separate
source-length cap bounds the four511-tap direct convolutions.

Independent controls compare the full513-band kernel, scalar convolution,
pooled phase and complete sampling equation. Scalar convolution error is
1.041e-17; the original three scalar render checks are at most1.388e-17.
Long-gap and continuous-source-with-cue checks add genuine two-run birth
coverage and a source cue at1300 without a phase reset, with error at most
1.997e-15. Nine independent capture checks include a last-frame impulse,
fractional rates and long nominal trailing silence. Four fractional combined
h=r identities, empty/silent inputs, immutable backing/caller mutation and
six malformed sources/five malformed profiles pass. Actual readonly large
inputs exercise memory and convolution-work refusal; tiny and minimum-subnormal
rates refuse capture allocation, and a real1e200 source triggers the phase-product
overflow refusal. Nonfinite phase-multiplier metadata is represented by an
explicit refusal with a finite/None report value.

The fixed first comparison uses unchanged impulse, mono900.37Hz and unseen_noise
sources at r1/k0 and r2/k-7,0,+7,+12: exactly15 complete responses. The final
comparison takes .850 seconds after its controls; preparation is about .02 seconds
per source in the initial run. All three unity and all three full-capture h=r2
identities are exact. Pre-event energy, final retirement energy and proportional
stereo error are zero for all15 responses, and every peak follows its source event.
All five mono tonal measurements pass15 cents with maximum .001091cent.

The first comparison nevertheless **rejects** the candidate. At rate2 the
q10/q50/q90 offsets in frames are:

| Source | k=-7 | k=0 | k=+7 | k=+12, h=r |
| --- | --- | --- | --- | --- |
| Impulse | 0/0/0 | 0/0/0 | 0/0/0 | 0/0/0 |
| Mono900.37Hz | -1/-3/-1 | 0/0/0 | -1/+3/+1 | 0/0/0 |
| Noise | +92/-139/-53 | +146/-76/+12 | +16/+7/+74 | 0/0/0 |

Noise/+7 passes the original timing predicates, but -7 and0 still fail the
unchanged96-frame q10/q50 limit. No larger matrix or parameter adjustment follows.
The even-born impulse has511 exact phase runs and its rate2 samples skip the
odd quadrature taps, so its pass alone is weak evidence for the phase action.
The mono and noise sources provide the needed nontrivial comparisons.

Passing the strongest-frequency test does not certify distortion: mono/+7 has
the desired1349.0304Hz peak at amplitude .22714 but also2252.4498Hz at .11233;
mono/k0 has900.3706Hz at .23322 and2701.1098Hz at .09549. Shifted noise output
energies are .37483/.37581/.37309 of source energy, against .50064 for the dry
h=r2 control. These values remain observations, with no fitted normalization.
The finite causal Q is imperfectly analytic; mixture cancellations, negative
images, real phase projection, underflow/fallback, fractional phase accuracy,
aliasing, finite live context, realtime and Human listening remain open.

Reproduce controls with
`uv run --no-sync python -B scripts/pitch_joint_residual_phase_controls.py`
and the frozen comparison with
`uv run --no-sync python -B scripts/pitch_joint_residual_phase_probe.py`.
They are explicitly invoked offline diagnostics. No existing v10 renderer is
replaced, and its baseline six timing failures remain. The new isolated/+7
observation is not product, Native or Human acceptance.

### Fixed Symmetric Quadrature With Joint Phase — 2026-10-17

`scripts/pitch_symmetric_residual_phase.py` evaluates one separate offline source
field. It mirrors the preceding fixed kernel antisymmetrically: qs[l]=q[l] for
positive l, qs[-l]=-q[l], qs[0]=0. The positive taps and cosine window are
unchanged; even taps and both final taps at +/-1023 are exactly zero. This is
Qsym=x+i*(x*qs), **not** the original513-band bank sum. Forward and reversed
calls to the unchanged causal preparation construct its complete convolution
without FFT, source labels or fitted gain, cutoff, window, weight, phase or delay.

The field has source origin -1023 and1023 source frames of lookahead. The same
pooled elapsed phase and single complex-sampling/phase-action equation use
u=r*(capture_origin+n)-field_origin. Nonempty nonzero input keeps
origin=min(0,floor(-1024/r)+1), nominal=ceil(N/r) and
retired=max(nominal,floor((last_nonzero+1023)/r)+1), using exact float-ratio
integer arithmetic. Empty/silent input keeps the original nominal rule.
Every negative coordinate, pre-event sample and positive tail is scored.
Causalizing by1023/r would add511.5 output frames at r2; this delay is never
hidden inside the unchanged96-frame timing limit. No live causality is claimed.

Independent controls pass: exact antisymmetric kernel with1022 nonzero taps,
two-sided scalar convolution error1.735e-17, three scalar phase/sampling errors
at most6.939e-18, four fractional h=r identities exactly zero, nine signed
capture cases, immutable backing/caller mutation and empty/silent sources.
Five actual refusals cover a readonly2M-frame source, the reachable halved
direct-convolution work cap, tiny/minimum-subnormal capture rates and phase-product
overflow from a1e200 source. The conservative256MiB logical preflight includes
both causal preparations and symmetric-copy overlap; it is not measured RSS.
The direct-work source limit is491520 frames. Existing numeric cancellation,
underflow fallback, aliasing and fractional-phase limitations remain explicit.

The shared first-comparison runner preserves the preceding causal experiment:
all22 JSON records, including15 profiles, are exactly equal after excluding
only top-level elapsed_seconds. The causal scalar-render helper adds an optional
field origin whose default remains zero. No existing renderer or scorer changes.

The same fixed15 responses now all pass the original measured predicates in
1.174 seconds after controls. Three unity and three full h=r2 captures are exact;
all five mono tonal checks pass with maximum .001091cent. Noise offsets at r2
are -7:[92,-49,8],0:[74,-4,10],+7:[95,-38,28],+12:[0,0,0] frames.
The +7 q10 pass is narrow against the96-frame limit. Noise energies/source
are .500037/.497865/.493622, against .500636 dry. Mono/+7's desired1349.0304Hz
amplitude is .250014; the2252.4506Hz line falls to .0000103. Mono/k0's
2701.1099Hz line falls to .00000882 beside the .250022 desired line. These
observations support the quadrature hypothesis without proving general quality.

Because the first comparison is useful, `--full` runs the existing18 sources
and four unchanged profiles, including target-blind dense close-pair ranking
and unrelated stereo. The terminal run renders all72 requests in3.498 seconds.
All18 unity captures are exact; original metric, signed-coordinate and source
spectral calibrations pass. The first wider invocation exposed a wrapper-only
KeyError after28 profiles because paired-background controls were not shared;
the wrapper now passes the same control dictionary across sources, as the
original scorer requires. The complete rerun is terminal with empty stderr.
Neither synthesis nor scoring predicates changed to resolve that wiring issue.

The wider matrix **rejects** the frozen candidate. Three original8ms-percussion
profiles fail the .1% isolated pre-event retention bound: fractions at -7/0/+7
are .345071/.277508/.120553, despite q10/q50 offsets only[-14,-11],[-11,-9],
[-6,-6] frames. No negative-time crop, amplitude threshold or fitted delay follows.
Twenty-one tonal profiles fail15 cents:

| Source | k=-7 max cents | k=0 max cents | k=+7 max cents |
| --- | ---: | ---: | ---: |
| Poly plus impulse | 1466.72 | 885.43 | 349.32 |
| Running background | 1374.53 | 821.34 | 320.37 |
| Independent900/1100 plus impulse | 536.15 | 289.21 | 102.31 |
| Unseen427.43/1327.89 | 1962.46 | 175.49 | 2115.80 |
| Close521.17/533.43 | 83.24 | 41.18 | 13.70, pass |
| Independently gated371.39/1381.77 | 2274.61 | 939.43 | 4186.93 |
| Unrelated stereo, left583.31 | 650.13 | 295.27 | 93.36 |
| Near Nyquist19873.23 | .0000044, pass | .0000329, pass | 850.00 |

The near-Nyquist/+7 request is rendered and aliases here; no new admission guard
is invented. This differs from the baseline's explicit refusal of that request.
Of36 tonal measurements,15 pass and21 fail. All three noise comparisons and
all close-pair timing predicates pass, but these are not tonal acceptance.
Proportional sources retain exact R=.5L. The unrelated source has no such
required relation; its observed R-.5L error is not a stereo-corruption verdict.
Busy-background comparisons remain nonisolated: tonal-attack pre-event output
differences reach .000540/.000380/.000660, and they certify no isolated retention.

There is also an algebraic limitation even for ideal quadrature. A single phase
factor preserves the sampled whole-mixture complex magnitude and stereo Gram
matrix pointwise. For ideal z(s)=exp(i*w1*s)*(1+epsilon*exp(i*dw*s)), small positive
epsilon, and k=h/r, its first-order action is
exp(i*k*w1*s)*[1+epsilon*(1+k)/2*exp(i*dw*s)+epsilon*(1-k)/2*exp(-i*dw*s)].
At s=r*t the sidebands are h*f1 +/- r*df, while independently transposed tones
require h*f1 and h*f1+h*df. Symmetry cannot fix that whole-mixture phase limit.
This is an ideal-case structural argument, not a fit or a proof assigning every
finite fixture failure to one cause; finite filtering and interpolation add risks.

Reproduce the controls with
`uv run --no-sync python -B scripts/pitch_symmetric_residual_phase_controls.py`,
the frozen15 with `uv run --no-sync python -B scripts/pitch_symmetric_residual_phase_probe.py`,
and the unchanged18x4 extension with the same probe's `--full` option.
No parameter search or further matrix follows this rejection. Production/API,
the v10 baseline and its six timing failures remain unchanged; finite live
context, continuous maps, Native/RT/resources and Human acceptance remain open.

### Public Exact-Model Complex-Gauge Bound — 2026-10-17

`uv run --no-sync python scripts/pitch_finite_am_gauge_controls.py` evaluates a
source-domain bound and twelve separately declared computed-basis examples. The
same controls now precede the canonical finite-AM quality matrix. The fitter,
actual all-pairs multiplier guard, source estimator, renderer and every original
threshold remain unchanged. This is not a replacement admission rule or a new
audio backend. Previous matrix, source-search and Native/API results are reused;
no full303-second ambiguity search, acoustic matrix or Native build is repeated.

For two feasible models write their fixed stereo I/Q basis as F, multipliers as
beta1/beta2, and full-source residuals as e1/e2. Let P be their common plateau
and U their contiguous union of nonzero gate support. When all four source-grid
landmark diameters are at most one frame, P contains the entire8192-frame
reference and U adds at most1024 endpoint samples to P. On P,
F*(beta1-beta2)=e2-e1. Thus, with epsilon=1e-4 and
K^2=lambdaMax(sum_U F'F)/lambdaMin(sum_P F'F),

`abs(beta1-beta2)/max(abs(beta1),abs(beta2)) <= 2*epsilon*K/(1-epsilon)`.

The numerator uses the two full-source residual bounds. The denominator follows
from (1-epsilon)*norm(source)<=norm(mode)<=sqrt(lambdaMax(G_U))*norm(beta).
No least-squares orthogonality, exact source factorization, isolated physical
interpretation or unobserved-model error estimate is needed for this inequality.

The public path is original carrier admission, AM grouping, immutable modulator
validation and finite fitting. The retained central frequency stays within
20..23520Hz and modulation spacing is at most11750Hz. Crucially, AM grouping
symmetrizes the sidebands around the original center. Those adjusted sidebands
are only guaranteed strictly inside0..24000Hz by modulator validation; they need
not remain inside the original20..23520Hz interval. The proof uses this actual
enclosing domain, not the stronger and unjustified sideband-floor assumption.
Manually supplied carrier estimates and low-level declared bases can have a
larger domain and do not receive this public-path theorem.

For unit pooled stereo direction C, let w=m^2, S_I=sum_I w and
T_I=sum_I w*exp(4*pi*i*fc*n/Fs). The Gram eigenvalues are
(S_I +/- abs(sum(C^2)*T_I))/2, with abs(sum(C^2))<=1. For modulation nu>=9Hz,
Dirichlet bounds at40,20,nu,2nu are respectively below383,765,1700,850. In the
T expansion both potentially near-resonant +/-2nu terms are conservatively
bounded by sample count, rather than assuming a sideband gap. With v=abs(u)<.5,
S-|T|>=N-E(v), S+|T|<=N*(1+4v^2)+E(v), where
E(v)=(1+2v^2)*383+4v*765+4v*1700+2v^2*850 <5930.
Consequently K^2 < [2*(8192+1024)+5930]/[8192-5930] and normalized multiplier
diameter is at most .000656423, below the existing .001 criterion.

For0<nu<9Hz put r=2*abs(u)<1 and shift modulation phase into x. Then
m^2=(1-r)^2+4r*(1-r)*sin^2(x)+4r^2*sin^4(x). All coefficients are nonnegative.
For each of the three weights, an interval's continuous integral is at least
its length times its maximum divided by8. Within one sine lobe, concavity gives
the stronger tent bound1/5. If one zero splits two arms below a crest, their
length ratio t reduces the fourth-power bound to
8*t^5-5*t+3>=0; its interior minimum is3-4*8^(-1/4)>0. If either arm reaches a
crest, its quarter-period integral3*pi/16 already exceeds the required bound.
The squared-sine case follows by applying sin^2>=sin^4 after normalization.

For N sampled consecutive frames, the Riemann/total-variation allowance gives
max(w)<=8*sum(w)/(N-17). For phase spans shorter than one modulation period,
Abel summation then bounds abs(T)/S by
32/[(8192-17)*2*sin(2*pi*20/48000)] = .747591046 <.748.
Longer spans contain k complete periods of length Tperiod>48000/9: their sum
is at least k*(3*Tperiod/8-4) and the Abel endpoint/variation numerator is at
most2*(k+2). The resulting ratio is below .575, hence also below .748.
These inequalities transfer to m^2 through its nonnegative decomposition.

Adding one endpoint yields S_(N+1)/S_N<=(N-16)/(N-24). Telescoping at most1024
additions gives S_U/S_P<=(1+1024/8168)^8<2.573. Therefore K^2<2*2.573/(1-.748)
and normalized multiplier diameter is at most .000903875. Constant modulation
has the smaller bound .000221763. All three exact sampled-model bounds stay
strictly below .001, independently of the original source phase.

This exact-model theorem is distinct from universal floating-point certification.
The existing cosine implementation can have cancellation near a deep slow trough;
no theorem about every libm/platform/extreme input is inferred from the algebra.
The actual computed-basis controls therefore form the finite I/Q Grams separately
using the program's public modulation evaluator, compare them with an independent
complex-moment identity, and include a conservative standard-dot-product rounding
allowance. Its numeric enclosure is conditional on those rounding assumptions.
No numeric result changes fitter admission or replaces the actual multiplier guard.

All twelve predeclared cases pass, including central-frequency limits, strong
slow troughs, nextafter(.5,0) depth,2**-35Hz modulation, long/late plateaus and
adjusted sidebands below20Hz or above23520Hz. Their largest computed-basis
distance bound is .000300849023; the largest relative Gram-identity error is
2.314e-16. Three out-of-theorem-domain inputs are rejected by this control's
proof-enclosure checker, not by a changed product or low-level fitter contract.
No source-fit search or standalone complex-refusal branch is executed here.

The exact public-model proof item is complete. Generic low-level complex
ambiguity, universal floating-point/cancellation coverage, cap-exhaustion evidence,
the full finite-gate evaluation and every product/Human gate remain open or
unclaimed. The candidate still has the same seven actual acoustic timing failures;
these source-domain bounds do not fix or conceal them.

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
`base_shift` (-23..+23), all 37 `extra_shift` values (-18..+18), and independent
piano/offset retrigger preferences. The combined total remains -23..+24; out-of-range
combinations refuse without clamping. Legacy absolute-root Learn actions retain
their original nearest -5..+6 mapping and fixed +6 tie.
Legacy `manual_key` delegates to this same table. Metadata correction remains independent;
unknown source text remains display metadata without guessed root or mode.
Failed deliberate analysis admission preserves correction; a genuinely admitted
request can remove its prior correction, while later corrections survive its
guarded result. Accepted replacement resets source-bound intent; restore retains
it. New explicit correction `None` prevents legacy resurrection on reopen and
malformed new fields recover locally without discarding valid performer settings.

The Piano button opens one responsive three-octave piano above the central PAD
grid. The opening state belongs to the UI and stays open across PAD changes;
selected note/octave, offset and both retrigger preferences follow the selected
content. Raised black keys have hit priority over the white geometry beneath.
The original detected root appears in the middle octave, marked separately from
the chosen note. A recognized metadata correction defines note labels and signed
targets consistently, while the detected original keeps its own separate marker.
Unknown correction metadata shows signed relative targets without guessing
an absolute note. These are key-root references, not detected first melody notes.
Valid learned, stepped or restored notes outside those fixed three octaves now
show their exact selected note, signed shift, relative octave and direction beyond
the displayed range inside the existing piano row. Unknown metadata remains
explicitly relative. No other octave is highlighted as a substitute; returning to
a displayed note restores its normal marker, without shifting the 36-key layout.

Piano and the offset row route GUI choices and learned MIDI/keyboard SET or
SET_RETRIGGER actions through the same Native `CapturedPitchAction` and Python
`PitchController`. `pitch.note` keeps signed octave identity; `pitch.note.step`
and `pitch.extra.step` resolve chromatic/octave deltas from the current Native
requested tuple, including rapid inputs. Legacy actions keep their meanings.
Original Key is one captured reset of piano note/octave, extra offset and manual
correction, retaining detected source metadata and unrelated controls. The offset
Reset changes only extra to zero. Each retrigger checkbox affects its own GUI
actions and newly learned variants; existing bindings retain their stored variant.
Old saved shared retrigger values initialize both preferences once on restore.

The collapsed MIDI / octave steps section selects CC step encoding for new Learn:
Button, Absolute encoder, Relative 1/127 (two's complement) or Relative 65/63
(offset binary). Choose the format actually sent by the controller. Button mode
uses a single high edge and ignores release/held repeats. Absolute mode uses the
first value as its reference and later modular direction; relative formats use
one signed tick per message. Learned action keys retain the selected encoding;
the selector never rewrites existing bindings or changes ordinary GUI/Note steps.
New piano-note and Original Key CC bindings also use one button high edge;
repeated NoteOn attacks remain independent. Use encoded step controls for an
encoder, and fixed piano-note controls for buttons or MIDI notes. Legacy
base/extra/base-reset CC bindings keep their previous behavior.

NRPN Data Entry retains the selected full parameter number instead of appearing
as generic CC6. Its seven-bit MSB carries the button or explicit encoder value;
the optional low byte produces no second input or attack. The same parameter may
send further values without repeating setup while its existing selection is fresh.
No complete fourteen-bit value is assembled. Without a complete fresh selection,
CC6 and CC38 remain ordinary CC inputs; channel, timeout, RPN and unrelated-CC
selection guards still apply. Existing NRPN Increment/Decrement retains Looper's
normalized 65/63 convention: choose Relative 65/63 for that input. This convention
does not mean the hardware sends 65/63 as the command's wire data byte.

The [MIDI Association Control Change table](https://midi.org/midi-1-0-control-change-messages)
identifies Data Entry MSB/LSB as CC6/38 and parameter selection as CC99/98. Its
[message overview](https://midi.org/about-midi-part-3midi-messages) describes selecting
NRPN before Data Entry or Increment/Decrement. Consuming the low byte without
assembling a high-resolution value is this application's bounded coarse-value
contract, not a claim of complete NRPN value support.

Device-free tests reproduce the old parameter130→CC6 failure, then exercise
freshness/channel/RPN/plain-CC guards and real raw NRPN Learn/save/reload through
the same production Controller, Native preparation, scheduler and PCM route as
CC. Parameters130/131 remain independent. All four Data Entry encodings retain
their saved SET/SET_RETRIGGER variants; fixed piano and Original Key bindings
ignore held/release/LSB messages and accept fresh repeated presses. These checks
retain the same unity-rate, virtual-clock and device/quality limits below.

The device-free production-route regression learns eight CC mappings using actual
Native MIDI events, writes a private mapping JSON and reloads it into a fresh
controller. Each encoding has a piano SET and an offset SET_RETRIGGER binding.
Later GUI checkbox/encoding changes leave these saved variants intact. Native
selection A -> CC -> B precedes polling; the retained original A owner then reaches
real preparation, the existing scheduler, effect feedback and PCM. SET preserves
the source cursor and adds no start; retriggers have distinct requests and attacks.
The bounded profiles cover both Key Lock modes and distribute the four captured
Quantize/SYNC pairs across the four encodings at unity source rate. They do not certify
the full encoding/timing/rate Cartesian matrix, NRPN devices, physical MIDI,
performer deadlines, DSP quality or Human acceptance.

The real mouse/controller/Native route exposed an admission serialization bug:
Python used integer zero for disabled Quantize, while Native requires `None`.
Default unloaded PAD rows also used zero, invalidating the entire atomic runtime
publication. Disabled Quantize now sends `None` for loaded and empty rows.
GUI capture refreshes same-frame source/metadata rows before admission; refresh
failure reports its actual reason. Learn intentionally consumes a choice for
binding and the central piano labels that state. The original reported Human
failure still requires an actual App retest; software evidence is not listening
or complete musical acceptance.

For the next Human App test, close the App yourself before rebuilding with
`start-release.bat --build-only`, then launch with `start.bat`. Codex's private
test build does not replace an extension held by a running App.

1. Load an analyzed source, select its PAD, open Piano and enable its Retrigger.
   Press different white and black keys, repeat one note, then choose the same
   root in lower and higher octaves. Each press should select that exact note
   and restart the regular loop beginning through the current Quantize/SYNC policy.
2. Leave Piano open and select another loaded PAD, then return. Opening stays
   stable; each content restores its own note, octave, offset and both preferences.
3. With piano C selected, offset +1 should show the nominal C# result while C
   stays selected. Offset Reset should restore only zero offset. Original Key
   should restore the detected root/original octave, offset zero and no correction.
4. Learn separate SET and SET_RETRIGGER bindings using the desired Note/CC/encoder
   input. Leaving Learn, exercise both; learned variants should retain their own
   behavior when either GUI checkbox changes. Watch the pending/applied/refusal
   status instead of treating a selected highlight as proof of audible adoption.
5. In MULTI LOOP, compare the source timing and the other PAD while changing notes.
   Notes must not change BPM or loop length. KEY LOCK OFF follows later speed
   changes, so the displayed note is nominal at nonneutral source rate.

Record actual device, musical quality, timing and listening failures separately;
these steps do not substitute for the remaining full acceptance gates.

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

### Productive successful pitch custody — 2026-10-17

`pitch_runtime_tests/productive_custody_workload_tests.rs` adds a separate
optimized Release measurement of the accepted custody path. Sixteen fresh
Windows children cover 1/2/4/6 audible voices, shared or distinct immutable PCM,
and both Key Lock modes at 48 kHz mono and actual r=1. Each child runs three
successful 64-action bursts through the real ImGui piano and routed raw MIDI,
1024-cell control/feedback queues, SourceACK, worker and existing 1024-event
scheduler. The original T/E, content and receipts stay fixed; each distinct E
produces one attack with its own prepared Native. Equal and opposing targets
remain separate. Each 65th action refuses without another work/Native allocation;
real off-callback retirement reaches zero claims and a fresh action succeeds.
All 195+voice-count successful receipts and three refusals remain retained.

The final 16 profiles pass (parent 32.44 s, children 1.78–2.13 s). Deferred direct
Native references verify every recorded sample, source phase, FIFO and generation
with maximum PCM error zero. Every cut also requires exactly the independently
born voice IDs, including uniqueness: an earlier review found that summing only
observed voices could miss a transiently absent peer, and that oracle gap was
fixed before this final run. Every measured callback observes zero Rust allocator
requests; the optimized current-thread calibration is checked separately.

All process snapshots below precede construction or execution of any reference
Native. They include Python, control/harness work and retained PCM. Peak values
are cumulative process-lifetime counters through that boundary, not per-cycle
peaks. MiB means 1,048,576 bytes; each row spans four topology/Key Lock profiles.

| Audible voices | Baseline working set, MiB | Lifetime peak working set, MiB | Lifetime peak private commit, MiB | Final private commit above baseline, MiB | Product-phase process CPU, ms |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 292.73–293.65 | 441.60–442.61 | 1162.49–1163.72 | 9.45–11.28 | 640.62–781.25 |
| 2 | 292.70–293.78 | 441.34–442.75 | 1162.11–1163.45 | 7.82–13.99 | 671.88–765.62 |
| 4 | 293.06–295.00 | 442.43–443.53 | 1163.00–1164.07 | 9.52–15.54 | 687.50–828.12 |
| 6 | 292.93–295.43 | 442.35–444.94 | 1163.03–1165.54 | 10.73–13.70 | 750.00–843.75 |

Retained output PCM is 276,000/308,772/374,316/439,860 bytes respectively.
Aggregate callback wall observations are 28.96–29.50/52.16–54.89/105.82–111.28/
168.85–179.12 ms and include allocator-instrumentation overhead. The output clock
is deliberately held during bounded worker waits, so none of these observations
certifies an 8192-frame performer deadline, C++ allocator behavior, device timing,
general continuous-O, variable-r, acoustic quality, complete P6/V0/216/30-minute
resources or Human acceptance. One-voice sharing and its same-loaded-slot MIDI
selection barrier are explicitly degenerate. Zero retired work claims does not
mean live voice or endpoint-reserve Native owners have been destroyed.

### Public piano and global-rate controller composition — 2026-10-17

`piano_rate_controller_tests.rs` joins the actual ImGui piano/raw MIDI route with
`UiContext` BPM-lock and speed controls, current source/timing ACKs, the existing
rate coordinator and sole scheduler. Four isolated GUI/MIDI × Key Lock OFF/ON
profiles use two complete-resident 48 kHz mono synthetic sources with accepted
Automatic periods .5/.375 seconds and physical Normal loops of 96,000/72,000
frames. Timing labels are declared fixture inputs; this is no inference result.
Python manual BPM values are cleared and accepted identity/revision checked so
the controller publishes the real coupled speed/master-period request.

At speed 2/master .25, A's +12 piano birth has actual r=2 and B's +7 birth r=1.5.
An original +7 action for A is genuinely ready before a public speed 1.8 request
changes the master period to .5/1.8. With Key Lock ON, A owns the first wet history
and the rate effect waits 4,096 frames; OFF retains its already productive Native
and applies immediately. Both effects precede the old pitch E. That action refuses
exactly `prepared_mismatch` at unchanged T/E, without an attack, generation change,
peer reset or rollback of confirmed timing. Controller status reports the existing
preparation error. A fresh same-note action then adopts its own prepared Native
and restarts only A at confirmed r=1.8; B continues at r=1.35.

Independent scalar interpolation/loop modulo, active 512-frame rate epochs,
direct Native history and per-voice filter chronology match every compared mixed
sample, FIFO, source phase, generation and actual Native pitch getter (`h/r` ON,
`h` OFF). The virtual input clock comes from the current Rust transport; a separate
piecewise beat calculation checks its master-period change. Each OFF/ON child
compares 43,069/34,877 total output frames respectively, including staged births.
Maximum PCM error is 2.9802322e-8. Debug four-child parent passes in 4.53 s;
optimized Release in 4.00 s. Adjacent Piano/Hold 9, unity-birth/rate 10 and
post-capture rate-refusal 1 pass. A fresh private Release and eight production
bootstrap API checks plus actual App/UI/controller/model imports also pass.

This complete-resident, bounded-history software proof does not certify finite
window storage admission, general continuous-O, all B5 values/trajectories,
acoustic quality, performer/device timing, P6/V0/216/30-minute resources or Human
acceptance. Worker waits hold the virtual clock; no App or device is started.

### Actual Native lifetime and complete callback-owner close — 2026-10-17

`pitch_callback_lifecycle_tests.rs`, stored beside `audio_stream.rs`,
uses the real boxed `AudioCallbackState`, shared production constructor, existing
control drains, renderer, sole scheduler and callback-close owner. Four isolated
children cover productive retirement and blocked complete close, each with Key
Lock OFF/ON, two nonneutral audible 48 kHz mono voices at actual r=1 and their
original Engine, source ACKs, captures, producer and work claims. Native references
process independent copied PCM outside marked render scopes; membership, source
phase, FIFO, generation and mixed PCM must agree. These actions use the public
Native capture/publication methods; the genuine GUI/MIDI controller routes are
covered by the preceding tests rather than reimplemented here.

A Windows-only test observer records each productive wrapper's successful C ABI
creation and completed C ABI deletion with a monotonic lifetime ID, actual Windows
thread ID and scoped render/close role. Pointer reuse cannot identify a lifetime.
Its hooks write bounded atomic scalar cells; snapshot allocation and decoding run
outside callbacks. A separate fresh-child calibration detects a deliberately bad
render role around a real deletion on the setup thread, with zero observed Rust
allocation requests from deletion/observation/role handling. This deliberate
negative control never runs inside a productive callback.

The existing retirement worker is held before consuming the normal 1,024-entry
queue plus 128-entry backlog. Releasing it before the original E preserves that
E and permits A's own prepared Native adoption. The displaced Native and a later
cancelled ready candidate are actually deleted on the measured live retirement
worker before terminal claims reach zero; the adopted Native and peer remain
productive. In the close cases, ready cancelled and public-Stop-fenced work stay
held behind full storage. Engine/cache/producer aliases are removed, then the
actual unique callback owner closes on another marked thread. All four named
Native lifetimes and both source weak probes survive until worker release. The
outer worker join completes callback destruction and nested worker shutdown;
every observed wrapper lifetime has exactly one completed deletion outside render
and close, work claims are zero and both productive PCM weak probes have no
readers, while four scalar receipts remain retained.

Debug calibration and all four lifecycle children pass; both optimized Release
parents pass in 1.02 s after a 1 min 56 s compilation. Every lifecycle child
observes 100 wrapper lifetimes/200 events; successful retirement compares 35,476
frames and blocked close 17,954 frames, with nonzero output energy and maximum
PCM error zero. Adjacent callback close 2, callback allocation 2, custody 4,
ordinary endpoint 23, allocator calibration 6, two-MiB constructor 1 and retirement
2 tests also pass. A fresh private locked/stripped Release build, eight existing
bootstrap API checks and actual App/UI/controller/model imports pass without
installing or constructing an Engine. The marked scopes execute the actual shared
drain/render kernels and complete close; they do not include CPAL timestamp/
telemetry/device code.
No C++ allocator/deallocator instrumentation, device deadline, acoustic quality,
general continuous-O/actual-r(n), full P6/V0/216/30-minute resource or Human
acceptance is claimed. No runtime defect or DSP correction was established.

## Inverse Seconds, Loaded Frames And Native Rate — 2026-10-16

`ScalarSourceGrid` evaluates the shared scalar projection in source **seconds**.
`VoiceSourceTiming::grid` instead multiplies an accepted period and origin by
the accepted loaded sample rate, so its inverse returns loaded **frames**.
Transport projects output frames through its output sample rate and precise
master period. Away from an explicit loop wrap, the dimensionless playback rate
is therefore `delta(source_seconds * FsLoaded) / delta(output_frames)`.
Equal physical progression at equal rates must give one; omitting the conversion
gives `1/Fs`, and applying it twice gives `Fs`.

The ordinary admitted loader prepares PCM at the configured output rate:
`material_migration::prepare_disk_pcm` calls the productive
`sample_loader/cold/stream.rs::prepare_playback_streamed`, which derives the
complete output extent with integer ceiling and writes an artifact tagged with
that output rate. Warm reuse checks the rate, transform, channels and extent.
The similarly named `cold.rs::prepare_playback` is a test-only helper.
`loaded_sample_shape` reports the stream rate, but that report alone is not
proof of buffer provenance; `SampleBuffer` has no independent rate field.

For hypothetical unequal loaded/output rates, physical progression instead
requires `r=(FsLoaded/FsOutput)*(source_period/master_period)`. This arithmetic
does not establish a productive unequal-rate playback route: ordinary preparation
already resamples to equal rates. Integer extent conversion and pinned delay
trimming also do not promise exact arbitrary-event phase below one output frame.
Configured sample rates do not measure physical device-clock drift or certify
stale cache metadata after a shutdown/device change.

The hardware-free `stretch_processor/coordinate_units_tests.rs` proof covers
44100/48000/96000 Hz, KEYLOCK off/on and +7 semitones at period pairs
`(.5,1)`, `(.5,.5)`, `(1,.5)` and `(60/119.999,60/137.123456789)`.
It admits the real captured pitch action first, then publishes current accepted
timing through the callback queue. Current content, source, permit and ACK checks
remain active. The old legacy period `.625` produces a demonstrably different
resolver target before the accepted publication; the acknowledged target then
agrees with the independent map rate.

The actual Python class/method is called via PyO3 outside rendering and worker
waits. Reduced integer fractions independently define physical source time from
the selected `Fs/10` birth and elapsed output time. Both absolute inverse anchors
are compared with that oracle, the accepted loaded-frame grid and the real voice
cursor. KEYLOCK modes are compared at the same declared three-quarter-second
output time followed by 2077 frames, without a wrap or fitted phase offset.
The stored Native pitch and the actual DLL getter must both report `h/r` or `h`;
generation and attack count stay unchanged. Missing and duplicate unit conversion
and using output-rate units for a hypothetical unequal loaded rate are excluded.
Nine separate unequal-rate arithmetic cases do not claim productive support.

All 24 integrated cases and nine arithmetic cases pass. Unit live rates are
exactly one; inverse-delta rates agree within `1e-11`. Native examples are
`2.996614153753363` for half rate/KEYLOCK on, `0.749153538438341` for double
rate/on, and `1.498307076876682` for KEYLOCK off. After the accepted domain
changes, the expanded half-rate/on profile initially lacks reconstructible past
history. The fixture advances actual post-publication playback only after a
preparation request is terminal, then lets the existing worker adopt at that
request's unchanged `F+4096`. Debug observed 7168 extra frames at 44.1/48 kHz
and 15360 at 96 kHz; these are software observations, not universal latency or
deadline guarantees. A new comparison made immediately after each differing
adoption time would compare different physical times and is explicitly avoided.

This closes the bounded units/rate evidence gap, without changing production
DSP, clocks, maps, public APIs, scheduling, Native history policy or gates. It
does not resolve any of the 13 rejected current-weight acoustic cases, certify
all 37 extras/base/total corners, variable trajectories, seams, finite handover,
continuous-output causality, realtime resources, devices or Human listening.

Fresh validation: the two new Rust tests pass in Debug (7.08 s) and Release
(5.53 s, optimized compilation 1 min 52 s), each executing all 24 integrated
and nine arithmetic cases. The impacted integrated checks pass: streamed loader
oracle one test/144 combinations (4.34 s), timing callbacks 10 tests (1.50 s),
source playback 17 tests (.01 s), and pitch runtime 54 tests (72.65 s) with its
one existing explicit Native-owner diagnostic ignored. That ignored diagnostic
is not counted as passing. Production/Python/ABI remain unchanged; no new wheel,
installation, App launch, device or listening check is claimed.

Reproduce from the repository in PowerShell, using the existing compatible
environment and Native build cache:

```powershell
$env:UV_NO_SYNC = 'true'
& .\scripts\run-rust-tests.ps1 -CargoArgs @(
    '--locked', '--release', '-p', 'flitzis-looper', '--lib',
    'coordinate_units_tests', '--', '--nocapture',
    '--skip', 'test_audio_stream_creation', '--test-threads=1'
)
```
