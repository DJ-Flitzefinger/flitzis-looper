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
Live or unobserved earlier owners retain their ordering barrier, including across
Piano and recording paths. Recording validates its exact pending capture/session,
geometry, cancellation and immutable admitted STOP before member requests change.
After every member binds, the same callback updates pending capture S to common E;
the existing event preserves OriginalOrder and its reserved STOP lane. A pre-start
cancelled recording cannot revive, and final member/capture claims remain atomic.
This one-shot-per-bundle path leaves unfinished workers out of the scheduler's deferred cell.
After a missed deadline, only genuine successful off-RT legacy reprepare rearms
completion, through a receipt bound to its exact rebuilt E. The worker releases
the prior displaced request; a new plan or successful bind consumes the receipt.
Every replanned group member must match that E before any bind/capture change.
Replan count and maximum launch delay remain bounded. Live earlier originals and
group-held deferred Work retain their ordering/custody paths.
Live SET, gliding/history-bound Piano and AttackAtGrid retain their existing paths. Piano uses
the exact retained start snapshot and a fresh action-owned permit/DSP intent;
actual or retained feed is excluded. Principal-attack DSP is
separate and the 30 ms goal remains
unmet.

The [performance audit](../development.md#performance-audit-and-upgrade-plan)
records production-route measurements and the remaining readiness/attack work.
The scheduling correction does not compensate the backend's principal attack;
the 30 ms software attack goal remains unmet on the declared synthetic burst.

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
