## Consumer and unit matrix (recorded before implementation)

| Consumer | Domain and precision | Origin/period and conversion |
| --- | --- | --- |
| Decode/resample | Original frames and original rate -> loaded frames at engine rate | Source origin stays zero; original rate is not the editor offset unit. |
| Saved intent | Python binary64 source seconds; integer loaded-frame grid offset | Independent activity/legacy base plus signed offset; manual/TAP precedes analysis BPM. |
| Visible scalar grid | Continuous beat coordinate -> binary64 source seconds | Independent signed source origin and effective period; loop selection changes labels only. |
| Snap | Nearest 1/16-beat coordinate -> physical loaded frame | Evaluate the absolute boundary, then round once; retain the existing midpoint convention. |
| Automatic loop end | Selected source start plus 4*bars beats -> physical loaded frame | Evaluate start/end from original selected intent, round each absolute endpoint once, never step rounded beats. |
| Waveform query | Binary64 source seconds -> clamped half-open loaded-frame range | Floor lower/ceil upper bound with exact frame-address tolerance; X is frame/rate in binary64. |
| Seek | Binary64 source seconds -> nearest valid loaded frame | Existing source-bound clamp and intro/loop/tail policy; no change to loop intent. |
| Playhead | Integer next-source cursor -> binary64 source seconds | Floor fractional cursor as before; preserve addressed frame through telemetry/PyO3/Python. |
| MIDI runtime | Authoritative effective BPM and precise loop/origin metadata | Signature uses the same effective resolver as publication; no raw-analysis-only cache key. |
| Native live source grid | Binary32 accepted BPM promoted to binary64; signed source-frame origin | Existing live phase/compatibility behavior remains; control evaluator accepts full binary64 BPM. |
| Output transport/scheduler | Integer output frames, binary32 master BPM promoted for phase math | Permanent Rust output clock; source display/seek precision does not redefine it. |

## Bounded arithmetic boundary

Extend the existing pure SourceGrid with a binary64 period constructor and forward
projection, retaining the existing live constructor's accepted binary32 BPM. Expose
that small evaluator through PyO3, wrapped by a focused Python scalar projection.
No accepted timing revision or second variable beatmap is introduced. Control callers
pass the full effective binary64 period (60/effective BPM). Automatic loops evaluate
both endpoints from the original selected start before independently rounding them;
rounding the start and then adding duration can move the intended end by more than
half a frame for legacy fractional-frame starts.

Continuous positions retain fractional frames. Physical markers round each absolute
position once to the nearest loaded frame; sample midpoint ties retain the current
Python nearest-even convention. Grid-point midpoint selection also retains nearest-even.
Seek retains its existing nearest-frame convention and valid-read clamp. Bounds that
originate as n/rate must not include/exclude adjacent frames through binary64 division
noise; any tolerance is limited to floating-point roundoff, not a musical correction.

## Verification

Use independent integer pulse truth n*24000 for all 1200 manual-120 positions at
48 kHz. Test 119.999/123.45 BPM, subdivisions, 44.1/48/96 kHz, signed/nonzero origins
and later/off-grid loops. Generated sparse address fixtures at 600/1800 seconds
verify one-frame reads, distinct consecutive X and seek/telemetry round trips
without allocating long dense buffers. An in-file private WAV probe uses 599.5 s.
Exercise untouched BPM edit commit/focus loss, deliberate edits, TAP/clear, metadata
replacement/restore and effective-resolver invalidation. Full native/Python checks
and official strict validation are required; numerical passes do not certify audible SYNC.
