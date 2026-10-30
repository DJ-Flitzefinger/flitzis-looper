## Reuse and bounded admission

GlobalRateCoordinator already owns requested/pending/effective rate state,
RateCapture/RateVoice, source permits, participant validation, preparation service
and commit_slot_write_fixed. Extend that authority instead of adding a direct
setter or a second transaction. Scalar-only behavior retains its existing paths.

For changed tuples with active mapped voices, admit only the explicitly qualified
all-dry cohort: mapped Key Lock off, total pitch zero, BPM Lock on, a present valid
actual master period, unchanged original source/map owners and supported whole-map
local rates. Refuse the complete effect for active Native ownership or retained
Native input as well as voices that need new Native history. Those are distinct
checks: an already productive Native can report needs_history=false.

Mapped pause/seek and unsupported transitions retain their existing gates. Capture
each genuine current reader/source permit, generation, placement, continuity and
effective tuple; do not reselect a later requested map. Keep a rate-specific mapped
snapshot entry or an explicit mapped-dry capture mode in the shared builder, so
generic live-pitch/Key-Lock/history callers do not accidentally gain admission.

## Actual boundary and one master

Factor rate capture's execution boundary. This dry effect uses E=F at the actual
settled callback boundary, not the Native F+4096 preparation interval. If command
and parameter drains are temporarily unsettled, retain Preparing and perform the
same capture/claim at the first settled boundary. Never bypass either fence or
convert temporary unsettled state into a fabricated source failure.

Use TransportTimeline.set_master_period_preserving_beat_position_at_frame at E.
Each mapped replacement keeps its original epoch marker and creative phase and
borrows the original geometry. SourcePlayback.replace_mapped_epoch must prove
bit-exact current canonical position. Preflight every replacement and reader before
the existing atomic group claim; install all playback replacements, global tuple
and transport together inside it. Owner, voice generation, original launch target
and output-frame clock stay fixed.

The existing accepted-anchor controller publishes speed plus anchor_period/speed.
The mapped source is S(M(n)+phase), using the actual master period once. A speed-only
request with unchanged actual period changes the speed observation but preserves
mapped coordinates and epoch. It must not derive an implicit new tempo. Repeated
effective tuples preserve the old epoch and pending targets.

After a real period change mapped dry coordinates follow the new M slope directly;
scalar dry peers retain the established .05/512 source-rate glide. Do not apply
that scalar glide to mapped coordinates or hide this transient distinction.
Actual period changes fence pending old-epoch starts; unchanged epochs do not.
New dry mapped starts/retriggers at nonunity speed still require the real qualified
epoch and ordinary original E=T claim, with no extra speed multiplication.

## Verification and remaining gates

Use genuine public parameter drains and source custody. Independently evaluate
piecewise master beats with declared change boundaries. Check each historical
read span against its own frozen epoch, not the current voice epoch. Prove exact
M(E)/s(E), stable original owner/generation/T/start count, new-rate continuation
through many knots/wraps and unequal caller partitions. Cover coupled fader,
speed-only, repeated rate, post-change start/retrigger/stop, finite fullmix and any
admitted stable stem readers.

Test whole-map out-of-range rejection without segment clipping, mixed Native
participants, stale source/window/ACK, unsettled drains, supersession/cancel and
pending old-epoch starts. Unsupported or stale effects preserve all preceding
audio, global timing and participants. Existing scalar/Native, ordinary-start,
history and equal-epoch controls remain required affected neighbors.

The speed domain is the actual supported continuous control domain, not an
invented set of 37 fader steps. Original Rate37/all37 extra -18..+18 and total
-23..+24 acoustic obligations remain unchanged and open. No listening/device,
perceived-onset, all-mode or full-SYNC acceptance follows from this dry proof.
