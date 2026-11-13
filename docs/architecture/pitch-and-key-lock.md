# Pitch and Key Lock

Source Key is classification and correction metadata. Piano and Transposition
set numeric pitch. Key Lock compensates for pitch changes caused by playback
speed. All three keep the [same source and clock](grid-and-source-timing.md).

## Performer intent

[`key_intent.py`](../../src/flitzis_looper/key_intent.py) owns a fixed
216-entry PAD table. Analysis/correction epochs are independent of base and
extra shift. The 36-key Piano sets base shift within −23…+23 semitones.
Transposition sets an independent −18…+18 offset. The sum must fit −23…+24
and unsupported choices are refused. Correcting the Source label keeps numeric
pitch. Original Key clears source correction and both shifts. Transposition
Reset clears only the offset. Unknown Key permits relative pitch without
fabricating a tonic.

A nullable per-PAD Key Lock override inherits Global when `None`.
Explicit ON/OFF survives a global edit. Choosing Global clears the override.
Save stores these intentions. Restore uses fresh source-bound preparation.

## Captured actions

| Intent | Native behavior |
| --- | --- |
| SET | Change pitch on the current trajectory while preserving position and running/paused/stopped state. |
| SET_RETRIGGER | Capture a fresh action and guarded launch, even for the same highlighted note. |

Piano Retrigger defaults to ON per PAD, including unset saved preferences.
Transposition has a separate preference. Learned bindings retain the variant
and target captured at Learn. The
[controller](../../src/flitzis_looper/controller/transport/pitch.py) and
[Native MIDI path](../../rust/crates/looper/src/audio_engine/input_mapping/pitch.rs)
preserve original input time, sequence and content ownership. The callback
rechecks current source, timing, window, pitch and preparation permit.

[`pitch_runtime.rs`](../../rust/crates/looper/src/audio_engine/pitch_runtime.rs)
uses the ordinary scheduler with 64 action slots, an 8192-frame preparation
lead for applicable late actions, and at most eight late replans. The original
target T stays fixed in its receipt. A first-sound birth or atomic group can
bind an earlier ready effective frame E at or after T without overtaking
earlier possible effects. An atomic group shares E and binds its recording
ticket when present. Live SET and history-bound or AttackAtGrid cases keep
their own preparation. A fresh physical birth emits its source beginning at
E and does not consume the unheard E−T interval as source history.

## Rate and DSP

With total shift `s` semitones and effective source speed `r`, the shift
factor is `h = 2^(s / 12)`. Native uses pitch scale `h / r` with Key Lock
ON and `h` with it OFF. OFF retains sounding ratio `r * h`. Pitch changes
leave source progression to its scalar rate. Exactly neutral Native pitch
can use the dry ordinary-start path.

[`rubberband_backend.rs`](../../rust/crates/looper/src/audio_engine/rubberband_backend.rs)
uses Rubber Band LiveShifter with the Short and ChannelsTogether settings.
Rate compensation steps by at most 0.05 every 512 active output frames.
[`key_lock_start_policy.rs`](../../rust/crates/looper/src/audio_engine/key_lock_start_policy.rs)
defaults to FirstSoundAtGrid with optional AttackAtGrid. Changes are
stopped-only and check pending owners. Requested target, effective render
frame, first significant sound, principal attack and device onset measure
different events.

## Preparation and actual feed

[`key_lock_preparation.rs`](../../rust/crates/looper/src/audio_engine/key_lock_preparation.rs)
warms the initial and reserve handles off the callback. The bounded worker pool
keeps original ordering and independent per-voice Native/FIFO owners.
The lower-priority selected endpoint has one preparation reserve.
Workers reset, prime, recycle and destroy handles. The callback can emit
bounded silence while a required reserve is unavailable.

[`prepared_native_history.rs`](../../rust/crates/looper/src/audio_engine/prepared_native_history.rs)
owns the prepared Native handle, FIFO and retained source/stem pins.
[`source_feed_history.rs`](../../rust/crates/looper/src/audio_engine/source_feed_history.rs)
keeps a bounded actual-read journal (1024 spans, up to 262144 frames).
SET and other continuation work replay actual eligible feed, including
source identity and interpolation. A Pause adds no active history. Normal,
Intro, Tail and [prepared stems](stems.md) require the corresponding whole
read context. Stale, unavailable and exhausted preparation can refuse an
effect before mutation. Retirement waits for queued, current, outgoing and
history readers outside realtime.

See the [time-stretch and pitch contract](../../openspec/specs/time-stretch-pitch-shift/spec.md)
and [archived render measurements](../../openspec/changes/archive/2026-11-13-improve-performance-isolation/design.md#step11-practical-principal-attack-assessment)
for the scope of past attack and latency runs.
