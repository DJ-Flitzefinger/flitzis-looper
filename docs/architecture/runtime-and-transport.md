# Runtime and transport

This module describes the current scalar audio runtime. Start with the
[overview](../architecture.md); media owners are in [media and PCM](media-and-pcm.md),
application intent in [state, input and UI](state-input-and-ui.md).

## Clock and execution owners

[audio_stream.rs](../../rust/crates/looper/src/audio_engine/audio_stream.rs)
creates the CPAL stream and callback state. Its callback owns
[TransportTimeline](../../rust/crates/looper/src/audio_engine/transport.rs),
[TransportScheduler](../../rust/crates/looper/src/audio_engine/scheduler.rs) and
[RtMixer](../../rust/crates/looper/src/audio_engine/mixer.rs). Output-frame time
advances for every rendered segment, including silence. Beat projection derives
from sample rate, a binary64 master period and a complete beat anchor. Changing
tempo preserves current beat position without resetting sample time.

The scheduler has 1024 event slots, absolute output-frame targets and stable
ordering. Original input captures and ordinary effects share one total ordering
domain. Full storage refuses admission instead of evicting earlier accepted
events. Rendering splits callbacks at due-event boundaries so the effect and
subsequent samples use the same frame authority. Late handling does not turn
Python wall time into a playback clock.

## Bounded control and observations

The native API validates inputs before publication. The callback drains at most
64 ordered control messages and 64 parameter messages per invocation from
separate 1024-entry rings. Parameters coalesce by identity within the drained
batch; the latest drained target for a touched identity wins. They do not replace
ordered source-changing transactions.

Controls include source/load, transport, loop, timing, pitch and prepared stem
transactions. Parameters include levels, EQ and rate targets.
[messages.rs](../../rust/crates/looper/src/messages.rs) defines boundary values;
operation-specific modules retain the source owners behind them.

Audio messages carry lifecycle events, peaks and playheads. High-rate telemetry
is best-effort. Durable tickets and callback-owned observations provide stronger
proof where required. [ordinary_transport.rs](../../rust/crates/looper/src/audio_engine/ordinary_transport.rs)
binds ordinary state to content lifetime, source epoch, revision and effect kind;
[pause_intent.py](../../src/flitzis_looper/controller/transport/pause_intent.py)
projects those observations even when generic lifecycle feedback was missed.
An accepted pause admission is pending until the matching Native outcome.

Atomic operations reserve complete effect/feedback/retirement capacity before
mutation. Rejected, stale, no-effect and applied outcomes remain distinct.
Neither a meter nor a queue receipt proves source adoption.

## Quantization and scalar musical time

[transport.rs](../../rust/crates/looper/src/audio_engine/transport.rs) computes
future grid targets; [global_params.py](../../src/flitzis_looper/controller/transport/global_params.py)
projects requested quantization. Fresh settings choose 1/16 with quantization
disabled; existing valid saved choices remain authoritative. Quantization chooses
a launch frame; source reading follows effective loop/explicit-seek policy.
It does not continuously follow another source's phase.

Already admitted events keep absolute targets and input order across tempo
changes. Current-source/intent validation still applies at execution: stop,
replacement or superseding edits can fence an original pending action.
Preparation retries cannot capture a different input or silently move its target.

Source-qualified BPM Lock and accepted timing refresh can qualify the internal
one-time transport bootstrap. It waits for a valid active reference, anchors
master beat position without moving playheads/resetting output time and does not
rearm on ordinary silence/wraps/stem changes. There is no general performer
phase-anchor control. See [accepted timing](../accepted-constant-timing.md).

[timing.rs](../../rust/crates/looper/src/audio_engine/timing.rs) uses one Rust
monotonic input epoch, captured at native MIDI entry or the accepted GUI gesture.
Checked CPAL callback/playback timestamps provide output-clock estimates.
Freshness, driver precision, downstream buffering and processing delay limit
them; they are not measured audible onset. Diagnostic nearest-grid targets do
not replace the current launch policy.

## Ordinary playback, groups and temporary holds

[playback.py](../../src/flitzis_looper/controller/transport/playback.py) and
[global_playback.py](../../src/flitzis_looper/controller/transport/global_playback.py)
share native guarded playback with mapped input. Original global captures retain
source-qualified membership through deferred preparation; polling cannot
recapture a new group or invent new original timing.

- MultiLoop ON permits concurrent PADs; global START/STOP uses its captured
  current group and transactional acknowledgement.
- MultiLoop OFF follows the last genuinely effective Native content/source
  target, including current placement, rather than editor selection. START alone
  may use selected loaded content as fallback when no effective target exists.
  STOP cannot manufacture that fallback.
- ON-to-OFF stops source-qualified peers atomically and preserves the target
  without retriggering/resuming it or replacing master tempo. Earlier pending
  originals are fenced by current hierarchy/order.
- Ordinary Pause preserves position; Resume continues the current source.
  A stopped/EOF voice is not a paused voice. Trigger/retrigger follows its
  admitted launch intent. OneShot reaches its real end; loops wrap their
  supported source domain.
- Temporary waveform Hold has its own captured owner, action/release protocol
  and cancellation in [hold_action.rs](../../rust/crates/looper/src/audio_engine/hold_action.rs)
  and the [mixer path](../../rust/crates/looper/src/audio_engine/mixer/hold_action.rs).
  Focus loss, release, hidden editor and close request settlement. Hold does not
  create a second ordinary Pause toggle or own refilled content.

[explicit_playback_target.rs](../../rust/crates/looper/src/audio_engine/explicit_playback_target.rs),
[global_playback_batch.rs](../../rust/crates/looper/src/audio_engine/global_playback_batch.rs)
and [ordinary_global.rs](../../rust/crates/looper/src/audio_engine/mixer/ordinary_global.rs)
own target/batch/effect checks. Arrangement fences prevent observers from
projecting intermediate participant placement as final playback truth.

## Rates, loops and rendering

[SourcePlayback](../../rust/crates/looper/src/audio_engine/source_playback.rs)
derives fractional source progression from active output-frame counts and a
binary64 rate epoch. Callback segmentation must not introduce repeated integer
rounding. [source_grid.rs](../../rust/crates/looper/src/audio_engine/source_grid.rs)
and [scalar_grid.rs](../../rust/crates/looper/src/audio_engine/scalar_grid.rs)
project the same scalar timing for transport/UI consumers.

BPM Lock is independent of MultiLoop. With valid source/master periods its tempo
ratio derives from source period / master output period; missing source timing
uses the supported global-speed path. Selection is not a new master.
[mixer/bpm_lock.rs](../../rust/crates/looper/src/audio_engine/mixer/bpm_lock.rs)
retains requested/effective state and source-bound master provenance; global
rate/history preparation can remain pending or be rejected.

[SourceReadPlan](../../rust/crates/looper/src/audio_engine/source_reader.rs)
is shared by dry reading, prepared history and stem trajectories. Physical
markers are integer frame bounds; musical loop period can be fractional. The
final PCM knot interpolates to loop zero at that period. Do not substitute
physical-length/musical-period into rate to hide rounding. Normal/Intro/Tail
reads require their whole admitted context; finite views keep full-source
coordinates and can refuse unavailable context.

Each [VoiceSlot](../../rust/crates/looper/src/audio_engine/voice_slot.rs) owns
mutable progression/processing state; immutable material may be shared.
Prepared stems follow the same trajectory with a bounded switching transition.
Pitch/Key Lock uses prepared source/history and Rubber Band, preserving source
and content binding. Source Key metadata is distinct from transposition.
Details and retained limits remain in [Key Lock](../key-lock-backend.md) and
[prepared stems](../prepared-stem-publication.md).

The mixer applies smoothed PAD Gain/Trim, PAD DSP/isolator and trigger velocity,
then sums and applies master level/momentary output mute. PAD meters are
pre-master; master meters reflect the output mix and expose finite clipping
above 1.0. [dsp.rs](../../rust/crates/looper/src/audio_engine/dsp.rs) owns the
bounded chain. Recording uses [mix_capture.rs](../../rust/crates/looper/src/audio_engine/mix_capture.rs);
file finalization/encoding remains outside realtime.

## Retirement, failures and evidence limits

[buffer_retirement.rs](../../rust/crates/looper/src/audio_engine/buffer_retirement.rs)
transfers large buffers, prepared work and retained owners off the callback.
Retirement/feedback pressure can prevent an effect before mutation. Cancellation
after claim needs the actual terminal outcome; a timeout cannot assume that
the callback never adopted the source.

Shutdown distinguishes real stream close from failed disposal/join or sticky
stream failure. Stop alone does not prove readers are gone; dependent owners
settle before teardown. Device failure, unavailable preparation and queue pressure
are visible failures, not promises of eventual playback.

Relevant regressions include ordinary transport, ON-to-OFF peer stop, pause
observations, Hold, scheduler ordering/atomic groups, fractional loops, source
replacement and retirement capacity. Current contracts cover
[transport](../../openspec/specs/transport-timeline/spec.md),
[MultiLoop](../../openspec/specs/multi-loop-mode/spec.md) and
[source/stem alignment](../../openspec/specs/loop-source-stem-alignment/spec.md).
The [loop proof](../loop-period-proof.md) and [device protocol](../device-loop-acceptance.md)
preserve separate rendered/device/listening limits; documentation and synthetic
passes do not substitute for their unpassed acceptance gates.
