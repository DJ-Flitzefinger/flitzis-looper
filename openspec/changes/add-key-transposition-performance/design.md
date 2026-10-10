# Independent metadata, pitch intent and admitted events

Status: K-META neutral policy/storage implementation; no current audible pitch
or new pitch UI support asserted.

Use existing ProjectState/PadController/LooperAction/input routing and Rust
source-reader/processor/preparation/scheduler ownership. Current manual_key is
metadata; current native RuntimeState lacks selected-content projection. Keys
currently use MIDI kind/channel/number without device identity. Extend authoritative
selection ordering for controller selection then keyboard pitch; retain existing
multi-port/channel collision rules and prove the two-device workflow rather than
invent current per-device binding support.

Persist recognized SourceKeyVersion, separate optional correction+epoch, numeric
base_shift, numeric extra_shift and retrigger flag. Labels derive from corrected
source root+base and root+base+extra. Correction/removal preserves both numbers;
successful deliberate analysis admission advances epoch and removes only the
preexisting correction. A correction made after admission wins against late results.
Failed admission preserves state; restore is not analysis. Accepted true source
replacement creates fresh lifetime and neutral source-bound values; copy/move/restore
preserve them. Legacy manual_key becomes metadata with base=extra=0, retriggerOFF.

For same-mode absolute target, d=(target-source) mod12, d>6 => d-=12, fixed +6 tie.
Thus base=-5..+6, extra all37 integers -18..+18, total k=-23..+24, h=2^(k/12).
For equal-rate existing varispeed route lockON p=h/r, lockOFF p=h with audible r*h.
At r=.5..2 this desires p~.132433..8; actual future r(n) extrema are part of B5.
Current .5..2 compensation clamp, lockOFF dry and positive-finite API checks are
not support. B5 establishes complete finite context/quality/RT/readiness/latency/
unity transitions. A concrete demonstrated limit may narrow one common validated
UI/MIDI/storage/audio domain with visible rejection, never silent clamping or tempo
changes. Old diagnostic corners remain regression cases, not replacement coverage.
All selected stems share one trajectory/k; input backing is immutable, per-content
DSP/voices separate. No added complete pitch PCM or serial second shifter.

An authoritative admission captures unique action/attack sequence, original input
timestamp, explicit GUI content or native selected ContentInstance+lifetime, full
base/extra/keylock/render tuple, source/timing/native/history/prepared permit and
existing trigger/startpoint/Quantize/SYNC intent. Pure polling/preparation is not
accepted action. Direct native/fallback/pending paths use the same envelope and
target; Move/Swap follows living identity, removal fences it. Capacity/unready/
unsupported failures are atomic and visible. Prepared work may share/coalesce;
each admitted attack remains its own bounded existing scheduler record and permit,
including equal-value repeated mouse/MIDI attacks. Later requests do not globally
invalidate an earlier admitted tuple. SET changes no cursor/start; SET_RETRIGGER
applies tuple and normal trigger at the same dueframe, never retunes early or toggles.

Checkbox selects GUI/new Learn variant; learned semantic variant stays immutable.
Absolute target actions encode pitch class/mode, relative ones signed integer,
not menu positions. Both variants for each target including0 coexist. NoteOff/vel0
does not attack/reset/highlight-clear or release a mouse waveform hold.

Both header-toggle menus stay open independently after selection/repeats/MIDI.
Render from common selected-content state, including closed summaries; keyboard
root is base-key root, middle0/signed steps, unique enharmonics, distinct -12/0/+12.
White/black are keyboard colors only; a contrasting selection survives NoteOff.
Result is nominal for lockOFF, no first-sample-note claim or green/Camelot/scale UX.

All source/trigger timing, M/B/S/phi, P/H, other pads and current routing remain
independent. Native ownership/cohort/hold integration is shared with arrangement;
no MIDI PauseHold is added. Final gates and exact row coverage are in the program.
