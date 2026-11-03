# Pitch and Key Lock

Source Key is classification/correction metadata; transposition is numeric
performer intent. Key Lock compensates playback-rate pitch coupling. They share
current source ownership with [media](media-and-pcm.md), one [runtime clock](runtime-and-transport.md)
and [state/input](state-input-and-ui.md), without another scheduler.

## Key, Piano and inheritance

[key_intent.py](../../src/flitzis_looper/key_intent.py) owns a fixed 216-entry
`PadKeyIntent` table: raw source Key/version, independent analysis/correction
epochs, base transposition and extra offset. Base is -23..23 semitones, extra
-18..18, with total -23..24. Unsupported exact combinations are refused rather
than silently clamped. Correcting the source label does not change numeric pitch.
**Original Key** clears correction, base and extra; extra reset clears only extra.
An unknown source label permits relative pitch without fabricating a tonic.
Detection itself is in [Key analysis](key-analysis.md).

[pitch_keyboard.py](../../src/flitzis_looper/ui/render/pitch_keyboard.py) renders
36 fixed keys/three octaves. Original-root indication is separate from current
selection; an exact out-of-range selection is shown directionally rather than
rebasing the keyboard. GUI/controller actions expose requested versus applied or
refused state. UI selection alone cannot retarget captured content ownership.

Global Key Lock is a default. In [models.py](../../src/flitzis_looper/models.py),
`None` in the per-PAD override means GLOBAL; explicit true/false survives later
global changes, including same-value edits. GLOBAL clears the local override.
[global_params.py](../../src/flitzis_looper/controller/transport/global_params.py)
updates inherited PADs, preserving explicit choices; unloaded entries normalize
to inheritance. Persistence stores intent, never Native handles/FIFOs/history or
an old acceptance permit. Restore requires fresh source-bound preparation.

## Captured SET and SET_RETRIGGER

[controller/transport/pitch.py](../../src/flitzis_looper/controller/transport/pitch.py)
captures `CapturedPitchAction` before preparation. Content lifetime, placement,
sequence and original input time stay bound through execution and rearrangement.
Native [input_mapping/pitch.rs](../../rust/crates/looper/src/audio_engine/input_mapping/pitch.rs)
and [pitch_steps.rs](../../rust/crates/looper/src/audio_engine/input_mapping/pitch_steps.rs)
retain the same semantic variants for MIDI:

| Intent | Effect |
| --- | --- |
| SET | Changes pitch on the current source trajectory, retaining its position and stopped/paused state. It does not implicitly launch. |
| SET_RETRIGGER | Captures a fresh original input/target on every press, including the same note; launches through guarded source/history preparation. |

Changing the GUI Retrigger preference does not rewrite already learned SET versus
SET_RETRIGGER bindings. Learn still captures instead of playing. Native-dispatched
events are not replayed by Python; fallback preserves the original action/time.

[pitch_runtime.rs](../../rust/crates/looper/src/audio_engine/pitch_runtime.rs)
uses the existing fixed scheduler, 64 action slots, 8192-frame explicit preparation
lead and at most eight late replans. Original requested T stays immutable; actual
effect E is explicit. Queue/preparation failure cannot be presented as application.
At effect, [pitch_actions.rs](../../rust/crates/looper/src/audio_engine/pitch_actions.rs)
checks execution frame, source/request/epoch, pitch/Lock, timing/window/publication,
Native history permit and current feed. Stale work cannot alter replacement content.

The 8192-frame lead is a scheduling cost when an already-due action needs worker
preparation: about 171 ms at 48 kHz or 85 ms at 96 kHz, before considering DSP
onset. The endpoint reserve covers one selected Normal PAD and is consumed on
use. Its pure physical-birth comparison excludes the future action's launch
coordinates, while the transferred request retains that action's fresh phase,
source permit and original T. A matching ordinary start can use it at a past T;
E is then the current callback. An atomic group can use this path when exactly
one member needs Native and every other member is dry/unity. Multiple wet members,
missing readiness and AttackAtGrid retain worker preparation. For ordinary fresh
starts, exactly neutral Native pitch bypasses preparation. Nonunity costs can
affect both rate directions;
slower-only perceived delay still needs separate attack/readiness measurements.

[completion.rs](../../rust/crates/looper/src/audio_engine/pitch_runtime/completion.rs)
can now accelerate a completed FirstSoundAtGrid ordinary or constant independent
Piano SetRetrigger physical birth, or an entire playback/recording group, before that
fallback deadline. It recaptures the complete
action request/permit at E=max(T, callback), checks the retained physical beginning,
source controls and DSP, and retargets the same scheduler event with its original
order. The displaced request remains in Work for off-callback retirement. Every
member, including dry peers, receives the fresh E-bound request; Native/FIFO state
remains independent. Pending earlier originals cannot be overtaken. Future T is
never advanced, and quantization OFF imposes no musical minimum or coalescing.
An earlier owner conclusively marked rejected no longer fences a later ready
original or playback group. Its old slot, queued event and worker/source/Native
custody remain reserved until normal cleanup; rejection cannot become an effect.
A conclusively rejected member also proves its exact retained atomic group
impossible. Polling attempts the other members' normal rejection CAS; only
actually rejected peers cease fencing later originals. A lost CAS retries on a
later poll. Missing owners, epochs and deadlines supply no such proof. This
propagation releases no event, slot, worker, source pin or capture custody.
During one bounded chronological completion pass, a live earlier original may
provide a receipt only after its whole fresh bind and exact queued-event retarget.
Later originals preserve captured gaps with E at least predecessor E plus their
original T difference; equal T adds no minimum. The actual retained header event
must sort before the proposed successor. Unknown, busy, unbound and deferred
owners remain barriers, and final execution still waits for every possible
earlier effect. This also covers Piano and atomic groups without coalescing
originals. Recording validates its exact pending capture/session,
geometry, cancellation and immutable admitted STOP before member requests change.
After every member binds, the same callback updates pending capture S to common E;
the existing event preserves OriginalOrder and its reserved STOP lane. A pre-start
cancelled recording cannot revive, and final member/capture claims remain atomic.
This one-shot-per-bundle path leaves unfinished workers out of the scheduler's deferred cell.
After a missed deadline, only genuine successful off-RT legacy reprepare rearms
completion, through a receipt bound to its exact rebuilt E. The worker releases
the prior displaced request; a new plan or successful bind consumes the receipt.
Every replanned group member must match that E before any bind/capture change.
Replan count and maximum launch delay remain bounded. A real deferred group retry
can reuse wholly group-held, unconsumed FirstSound physical births through
[deferred_group.rs](../../rust/crates/looper/src/audio_engine/pitch_runtime/deferred_group.rs).
Exact member/header metadata, no possible predecessor, whole fresh source/permit/DSP
requests and capture/immutable STOP fences precede every later-E bind. All binds
succeed before common E/capture S changes; the same popped event retains ordering
and capacity. Occupied displaced-request pins and unavailable proof retain genuine
legacy reprepare. Each later bind consumes one existing replan allowance.
Live SET, gliding/history-bound Piano and AttackAtGrid retain their existing paths. Piano uses
the exact retained start snapshot and a fresh action-owned permit/DSP intent;
actual or retained feed is excluded. Principal-attack DSP is
separate. The Human clarified that 30 ms is advisory: seek the shortest practical
latency while preserving quality/functions and proportionate complexity.

The [performance audit](../development.md#performance-audit-and-upgrade-plan)
records production-route measurements and the remaining readiness/attack work.
The scheduling correction does not compensate the backend's principal attack;
the declared synthetic burst retains its measured attack displacement. The
advisory 30 ms reference does not require disproportionate redesign.

For a fresh ordinary PAD or Piano retrigger birth, the physical selected beginning
is emitted first at actual E. Unheard E-T is not consumed as source/filter history.
Source phase counts actually emitted frames; this full beginning does not claim
alignment with original T or peers. Atomic ordinary groups share actual E.
SET, resume and real existing-source replay retain their own actual chronology.
Rate/smoothing state is evaluated at E; resetting source phase cannot invent a
new rate epoch or restart the smoothing interval.

## Scalar rate and Rubber Band

[pitch_intent.rs](../../rust/crates/looper/src/audio_engine/pitch_intent.rs) derives
`h = 2^(total_semitones / 12)`. For effective source speed r, Native pitch scale is
`h / r` with Key Lock ON and `h` with it OFF. Numeric transposition does not alter
source progression; OFF therefore retains speed-related output pitch `r * h`.
There is no legacy compensation clamp. Valid finite arithmetic is still distinct
from available source/history, successful preparation and musical quality.

The productive backend is Rubber Band LiveShifter through
[rubberband_backend.rs](../../rust/crates/looper/src/audio_engine/rubberband_backend.rs),
pinned Rubber Band 4.0.0 with the required C Live API. Exact binary64 pitch is
passed through; nominal Native delay is not measured audible alignment. Expanded
upward cold initialization uses a silent unity block before the exact target;
every real source block uses the requested pitch. Settings expose no backend
tuning surface. Tempo-ratio changes step by at most 0.05 every 512 active output
frames; callback partitions and pause do not consume a different smoothing clock.

[key_lock_start_policy.rs](../../rust/crates/looper/src/audio_engine/key_lock_start_policy.rs)
defaults to FirstSoundAtGrid, with optional AttackAtGrid. This synchronous policy
change is stopped-only and checks voices, scheduler, Hold and pending owners.
It is not a queued live/next-start change. First sound, principal attack, requested
grid target and audible device onset remain separate measurements.

## Preparation, actual feed and retirement

[key_lock_preparation.rs](../../rust/crates/looper/src/audio_engine/key_lock_preparation.rs)
prepares a per-voice warmed initial handle, warmed reserve and source-only adapter
off realtime. Capacity-one SPSC lanes hand off work. Reset, source priming/catchup,
recycling and destruction stay on workers; cancelled results retire even when a
voice is paused or inactive. The callback never waits/spins for a reserve.
Unavailable reserves can produce bounded silence without rewriting canonical feed.

[prepared_native_history.rs](../../rust/crates/looper/src/audio_engine/prepared_native_history.rs)
owns unique handles, fixed input/output FIFOs and source/history/stem pins.
[stretch_processor.rs](../../rust/crates/looper/src/audio_engine/stretch_processor.rs)
binds actual source, selected/outgoing stems, read plan, `SourcePlayback`, accepted
timing and Native history permit. Logical playhead position is distinct from the
actual DSP-input trajectory. [source_feed_history.rs](../../rust/crates/looper/src/audio_engine/source_feed_history.rs)
records actual reads in a bounded journal (1024 spans/262144 frames), fencing gaps,
refill changes and stale source state. Pause adds no active past. SET prepares
from its own immutable snapshot and workers replay real feed chronology.

Dry, history and stem readers use one [SourceReadPlan](../../rust/crates/looper/src/audio_engine/source_reader.rs).
Normal requires loop context, Intro its prefix through loop end, Tail the real
remainder and return; ALL/complete residency needs separate admission. Same-phase
cuts preserve source taps and interpolation weight, reserve before cut and refuse
unavailable context. Fractional musical period and integer physical endpoint are
different. Source position conversions use source seconds/loaded frames with the
actual scalar transform, not an inverted rate or rounded-loop correction.

Own-source/voice/history permits and independent mutable DSP state remain per
attack, even with shared immutable PCM. Large buffers, source pins and Native/FIFO
owners retire off the callback after queued/current/outgoing/history readers end.
Stop, selection or a timeout is not last-reader proof. [Stems](stems.md) adds
candidate/ramp ownership without replacing this trajectory or its guards.
`key_lock_source_preparation.rs` is a test-only proof fixture, not a productive
mixer/scheduler preparation owner.

## Dependencies, evidence and current limits

The [STEP11 performance assessment](../../openspec/changes/improve-performance-isolation/design.md#step11-practical-principal-attack-assessment)
retains the current Short + ChannelsTogether backend and silent physical birth.
In 36 uncropped synthetic captures, Medium's principal attack is later in every
matched pair; nominal-delay cropping can remove substantial response energy.
Removing only the adapter's 511-frame packing lead is a separate potential design,
requiring new physical-birth actual-feed, live-control, completion and last-reader
proof. It is not implemented or claimed as a speedup. This bounded no-change
decision preserves current wet continuation; it establishes neither technical
optimality nor whole-quality or listening acceptance. The Human's 30 ms suggestion
is advisory, with practical benefit and proportionate complexity controlling further work.

The explicit `paced_group_matched_rate_key_lock_response` diagnostic compares two
cold 48 kHz mono groups: a rate-0.8 PAD and a unity peer, with Key Lock OFF and ON.
Both controls use actual source adoption, global capture, publication, conditional
worker preparation, scheduler and the same-render per-PAD capture. OFF with neutral
Native pitch skips that preparation. The test verifies the adopted Lock
and active source rates, reconstructs the mix and measures each attack relative
to its own effective frame E. OFF is the existing varispeed route; its pitch is
intentionally different, so aligned PCM equality is not a quality oracle.

For a fresh physical birth, `OrdinaryStartWork::prepare` calls
`prepare_source_at_physical_birth`. The retained-start path feeds silent history
without advancing the chosen source endpoint, then transfers the same Native and
FIFO to rendering. This prepares state before E while preserving the real source
beginning; it does not automatically remove processing displacement of the burst
first supplied at E. Readiness, E, first significant output and principal attack
therefore remain separate. The matched comparison diagnoses the additional wet
displacement; it does not justify cropping valid source beginnings or certify
device, whole-matrix, listening or quality acceptance.

The explicit `paced_other_rate_cold_piano_response` diagnostic adds cold public
neutral Piano OFF/ON controls at 44.1/96 kHz and ratios 0.8/1.16. It verifies
actual Lock, active source rate and retained source identity/extent, reconstructs
the same-render mono output and records attack landmarks after each action's
own E alongside host brackets. See the [scoped observations](../../openspec/changes/improve-performance-isolation/design.md#other-rate-cold-piano-controls).
Existing 48 kHz Piano observations remain historical where later shared
completion changes lack path-specific compatibility proof. Neither an unchanged
test body nor these eight new cells establishes whole-matrix, quality, overlap,
device or Human acceptance.

The explicit `paced_faster_and_other_rate_group_response` diagnostic extends
matched Key Lock OFF/ON observations to faster 1.16 groups at 48 kHz with 2/4/6
PADs and cold six-PAD groups at 44.1/96 kHz. Two members change source rate;
remaining members stay at unity. Warm selects only PAD0: ON awaits its Native
endpoint, while neutral OFF uses selection only without Native preparation. Actual source
rate bits match across controls, with each individual attack measured after its
own E and same-render source/mix checks retained. See the [scoped group observations](../../openspec/changes/improve-performance-isolation/design.md#faster-and-other-rate-group-controls).
Path-specific qualification permits the exact older ordinary/Piano fixtures'
source/DSP landmark reuse; their T/E and host costs remain historical N1.
The explicit `paced_bpm_and_partition_group_response` adds twelve six-PAD
controls for actual BPM Lock OFF at 512 frames and BPM Lock ON at 128 frames
or the cycle [128, 256, 64, 1024]. Cold and selected-PAD0 cases use actual Key Lock
OFF/ON; BPMOFF asserts unity source rates and requires no neutral Native endpoint.
Every actual span, bounded capture extent, source identity and own-E landmark is
retained. See the [scoped partition observations](../../openspec/changes/improve-performance-isolation/design.md#bpm-authority-and-callback-partitions).
The explicit `paced_slower_other_rate_and_single_faster_group_response` adds eight
six-PAD controls: cold slower 0.8 groups at 44.1/96 kHz and exactly one faster
1.16 member at 48 kHz, cold or selected-PAD0. Both actual Lock states retain the
unchanged group helper, source/adoption/capture guards and each own-E landmark.
See the [scoped rate-topology observations](../../openspec/changes/improve-performance-isolation/design.md#slower-other-rate-and-single-faster-group-controls).
Further ordinary/exclusive/global states, Piano retrigger/live SET, pause/Hold/seek,
unobserved rate/count/selection combinations, real background overlap, final
stress, device, quality and Human acceptance remain open. Internal source-read
domains do not imply a separate Normal/Intro/Tail/OneShot performer selector.

The explicit `actual_controller_routes_with_conversion_and_load` diagnostic adds
actual ordinary exclusive/peer-preserving and global-current-group controller
routes using complete three-second stereo sources. Actual selection differs from
the global target group. Productive guarded artifact conversion makes complete
call/worker-CPU progress inside adjacent paced playback periods while
two PADs play; all five numeric fixture outputs are verified. A real separate cold
import receives its original source ACK, with exact Loader CPU overlap still
unproved. See the [scoped controller observations](../../openspec/changes/improve-performance-isolation/design.md#actual-controller-routes-beside-artifact-conversion).
This neither runs inference nor closes further source states, the full overlap
matrix, device, quality or Human acceptance. Production DSP remains unchanged.

Rubber Band Live symbols must be available before audio starts. Public build
discovery uses documented pkg-config/environment/vcpkg routes; Windows DLL setup
and the Rust test wrapper are described in [development](../development.md).
Portable must deliver its required runtime independently of private developer
bindings. No DLL loading, file I/O, allocation-heavy preparation, blocking, GIL,
logging or model work belongs in the callback.

The [canonical E-PITCH evidence](../../openspec/changes/archive/2026-10-30-evidence-index.md#e-pitch)
retains the full backend report, negative onset/retention results and test-only
AR2/WSOLA/phase-gradient/AMFM/chirplet/noise experiments. Those experiments are
not alternate production backends. The isolated 48 kHz/rate2 case needs at least
1598 frames/33.292 ms causal headroom; that necessary bound is not a universal
latency guarantee. Native suffix/A-reset agreement does not establish equivalence
to a continuous voice-birth trajectory. Strict 2 ms/peak/energy failures, ambiguous
principal attacks and finite-history refusals remain retained.

Software onset chronology, bounded source/history/domain and inherited-override
tests do not pass whole quality/unity-transition, acoustic/device, visual or Human
listening gates. Unproved past-T glides, paused pending glides, unavailable read
domains or exhausted preparation capacity remain refusals. The optimization target
is 2/4/6 PADs, synthetic stress at most 10; historical 20/32 observations do not
create new performance promises. Current [pitch](../../openspec/specs/time-stretch-pitch-shift/spec.md),
[input](../../openspec/specs/input-mapping/spec.md) and
[persistence](../../openspec/specs/project-persistence/spec.md) contracts apply.


### Actual Loader-worker CPU observation during playback

The explicit Windows-only ignored diagnostic
`held_lifecycle_tests::loader_overlap::actual_loader_cpu_during_active_playback`
uses the real public Loader, cold-job admission, complete source publication and
headless callback fixture. Two full three-second sources remain active while a
stopped fourth PAD imports a complete sixty-second 44.1 kHz stereo source into
the existing 48 kHz stereo output. That same incoming request explicitly selects
manual ALL (start zero, absent end) through the public incoming-loop-intent API;
the adopted Project settings and complete residency are checked. PAD 2 stays
selected and stopped. The original
Loader request, Native source acknowledgement and original-file digest are
checked by the existing fixture. Full resident frame extents are checked, and every resident PCM scalar is hashed
for the raw record. The playing sources' hashes and original allocations are
compared with their initial values.
Stereo/divisibility follow the unchanged verified output-format and residency
contracts; the raw source tuples do not separately log channel/scalar counts.

A test-only TLS observer transfers with exactly the admitted job and is restored
when that worker scope ends, including unwind. Both ordinary and incoming cold
PCM callbacks record producer progress before the existing 100 ms UI-event
throttle. Actual Windows thread ID, creation time and `GetThreadTimes` CPU values
are independent of UI receipt times. A common `Instant` epoch brackets both CPU
queries conservatively. No producer gate, artificial work, sampler thread or
realtime callback instrumentation is added.

The original software sample recorded 4886 producer points and
6 of 95 consecutive active playback periods containing
a complete same-request, same-worker, same-stage advancing producer interval
with an observed positive worker CPU-counter delta. Every playback step advanced
512 frames. The maximum whole drain/allocation/render harness bracket was
0.285100 ms, with 0
brackets longer than the nominal 512/48000 interval. The maximum paced playback
period was 49.152100 ms; 43
periods exceeded nominal duration. Raw producer/period/identity/source rows are
emitted before derived assertions. Engine shutdown joins the actual cold workers,
their active/queued/admitted counts are all zero, and callback retirement is
observed before source-directory destruction.

This is one observed Loader-worker CPU-counter/progress sample within active,
paced software playback periods. Counter-accounting granularity, ordinary
reporting, observer overhead, I/O and cancellation checks are included. It does
not isolate decoder/FFT CPU, prove CPU inside the short render call, establish a
device deadline, exact uninterrupted voice phase, neural overlap, distribution,
hearing, quality or Human acceptance. Existing conversion and controller/matrix
proofs retain their original scope and samples. No production DSP/API behavior
changes, whole-matrix completion or practical Torch replacement are claimed.


### Actual single-mode global target controller bridge

The explicit Windows-only ignored diagnostic
`held_lifecycle_tests::controller_overlap::actual_global_off_preserves_explicit_target_across_selection`
loads three complete three-second 48 kHz stereo sources through the real public
Loader and retains their original Native acknowledgement, source/content identity,
file digest, resident PCM extent, allocation and full PCM hash. With MultiLoop OFF,
it starts PAD 0 through the actual controller, changes both Project and Native
selection to PAD 2, and uses global STOP. PAD 0 stops with an accepted Native fence;
its explicit target history survives and the selected PAD stays stopped.

After selection changes to PAD 1, the actual global START route creates a new
original PAD-0 capture. Its timestamp, sequence, content key and original T are
retained through application. Single-frame callback steps observe its own E,
matching execution/effective frames and original launch reference. PAD 0 has a
new voice generation and finite nonzero PCM; PADs 1 and 2 remain stopped. Actual
feedback agrees with the voice state, all loaded sources remain unchanged, and
Native bindings remain current. Final stop and actual engine shutdown join the cold workers
with zero active/queued/admitted jobs; callback retirement is observed before the
temporary source directory is released.

This is one functional controller-to-Native bridge, with no production DSP/API
change. It does not establish latency, device, Piano, pause/Hold/seek, neural,
whole-matrix, hearing, quality or Human acceptance. Existing scoped explicit-target
failure, source-replacement, peer and retirement evidence remains separate.


### Actual loaded Piano controller bridge

The Windows-only ignored diagnostic
`held_lifecycle_tests::piano_bridge::actual_loader_piano_set_and_repeated_retrigger_preserve_peer`
connects the real public Loader and Project source owners to public Piano SET and
SET_RETRIGGER actions. Two complete three-second 48 kHz stereo sources retain
their original Loader acknowledgement, content/source identity, file digest, full
resident PCM hash and allocation. Both PADs advance; Project and Native selection
stay on peer PAD 1 while Piano explicitly targets PAD 0.

Each action retains the newly created original controller capture before polling:
sequence, timestamp, content key and original T survive through its own actual E.
The diagnostic checks no early pitch or generation effect, exact source chronology
for SET, and a new generation at the loop beginning for each same-note RETRIGGER.
The peer retains its generation, source and Native state and advances throughout;
its actual post-DSP contribution is nonzero. Both PADs contribute finite PCM,
feedback agrees with active voices, and current source bindings survive. Final
stop, cold-worker join with zero jobs and observed callback retirement precede
temporary source destruction.

This is a bounded functional bridge, without a production DSP/API change. Its
software callback clock and PCM checks do not establish device timing, latency,
hearing, quality, neural or full-matrix acceptance. Existing GUI/MIDI and physical
Hold tests remain separate scoped evidence. The ordinary Loader pause/seek/resume
and loaded Hold/retrigger/original-release bridges below provide separate bounded
histories; all original acceptance gates remain open.


### Actual loaded ordinary pause, seek and resume bridge

The Windows-only ignored diagnostic
`held_lifecycle_tests::pause_seek_bridge::actual_loader_pause_seek_resume_preserves_peer`
uses two complete three-second 48 kHz stereo sources from the real public Loader.
Project and Native selection remain on peer PAD 1 while public ordinary pause,
resident seek and resume target PAD 0. Original pause/resume admissions are retained
before polling, then matched against actual content, revision, effect and outcome.
The source-qualified queued commands and their actual callback drain frames are
observed separately; an admission alone never proves an effect.

Pause freezes the target cursor, generation, Native allocation, FIFO and productive
DSP history while the peer advances and contributes actual post-DSP PCM. A paused
seek to source frame 36000 retains its actual residency ticket before polling.
The original capture only pins the voice; a later resident relocation must adopt
the seek before its ticket reports accepted, current and the effective target.
The callback voice and the subsequently acknowledged Session cursor must agree.
No pitch-action timestamp/T or ordinary seek admission is invented for this route.
Explicit seek deliberately increments the voice generation on the same lane and
immutable PCM allocation, resets stretch/FIFO and DSP history, and retains pause
and loop markers. Its first paused render reinstalls the unchanged physical loop
domain without advancing the cursor; the complete expected playback state is
checked across this transition. Resume preserves that new generation and advances from the
adopted position with real PCM. It establishes explicit playback target PAD 0
while editor selection remains PAD 1.

Full resident source hashes, allocations, Loader acknowledgements, content identity,
bindings and loop markers remain checked. Final stop, actual cold-worker join with
zero jobs and observed callback retirement precede temporary source destruction.
This bounded software callback history does not establish device timing, latency,
hearing, quality, neural or full-matrix acceptance. No production DSP/API change is
implied; existing controller, source-replacement, Hold and Piano proofs remain
separate scoped evidence.

### Actual loaded Hold, Piano retrigger and original release bridge

The Windows-only ignored diagnostic
`held_lifecycle_tests::piano_bridge::actual_loader_hold_retrigger_and_original_release_preserve_peer`
combines a real public Loader physical Hold with public Piano SET_RETRIGGER and
the original physical release. Two complete three-second 48 kHz stereo sources
retain their Loader acknowledgements, content/source identity, file digests,
resident PCM hashes, allocations, loop markers and current bindings. Project and
Native selection remain on peer PAD 1 while Hold and Piano explicitly target PAD 0.

The original Hold ticket precedes its actual pause acknowledgement; the original
Piano capture precedes polling. Until that action's own E, the held voice retains
its full playback state, generation, Native allocation, FIFO and productive
pitch/DSP histories while the peer advances and contributes actual post-DSP PCM.
At E, the retrigger adopts the exact prepared Native state and original request,
creates one new generation at position1 and preserves original T. The Hold becomes
Superseded at E, retaining its original pause effect with `effect_current=false`.

The original physical release precedes controller terminal polling and must leave
the new attack's full playback state, Native allocation, FIFO and productive
histories unchanged. Subsequent renders check its trajectory, actual target PCM
and continued peer contribution. Feedback, source custody, final stop, actual
cold-worker join with zero jobs and observed callback retirement remain checked.
This is one bounded functional history, with no production DSP/API change or
device, latency, hearing, quality, neural or full-matrix acceptance. Existing
SET, Key Lock, MIDI and release-before-E proofs remain separate scoped evidence.
