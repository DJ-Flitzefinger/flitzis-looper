# Content lifetime and native mutation linearization

Status: R0 target only. Current source uses slot-only pause/resume and pad-bound
stem eligibility. Reuse asset_lifecycle/project_assets/cold_store/stem_cache and
existing command/poll/ACK/journal paths; no framework or new scheduler.

PadSlotId is fixed0..215 controller layout; slot epoch changes with successful
occupant mutation. ContentInstanceId/lifetime owns mutable musical intent and
living native playback; lifetime never reused across restore/reassign/ABA.
Persistence preserves durable ContentInstance lineage and musical intent, then
reopen allocates a fresh runtime lifetime. Saved lifetime/action/feedback/hold
tokens are historical data and cannot regain authority over reopened content. Shared
immutable Material/Source/Analysis/StemSetVersion identifies data, not a pad or
PauseEffect. SourceTicket/current_binding/NativeHistoryPermit/source/timing/window
authority is current per-content proof, not copied historical evidence. R2 must
prove native remap including sample routing/SourcePlayback/DSP/history/FIFO/filter/
warm processors/GainEQ ramps/loop/STOP guards/pending results/telemetry; changing
Python arrays or voice.sample_id alone is insufficient.

CopySnapshot captures source, valid analysis, selected available StemSet, loop/
excerpt/grid/timing/manualTAP, source correction/numericbase/extra/KeyLock/other
playback/GainEQ/current musical mask/custommask/preset/mutes/retrigger checkbox.
Mutable settings become independent; immutable disk/input backing stays shared.
New copy is stopped with fresh current guards/native ACK, no cursor/voice/DSP
history/meters/progress/pressed/HoldAction/tempjobhandle/runtime-token clone.
Same suitable resident ranges share input; varied ranges have independent views;
each content retains independent DSP. Save/reopen after origin bank deletion uses
canonical material and fresh authority, no valid-data reanalysis/decode/separation.

Prepare one guarded immutable transaction for pair Move/Swap/Copy-overwrite or
all36 BankCopy/BankClear, including empty source target removals. Capture exact
source/target lifetime/slot/config revision, desired selection, all refs/job interests,
command/action/feedback/retirement/DSP capacity and journal recovery state. Acquire
all new refs before releasing any old. Pre-claim cancel/failure is complete no-op.
Native execution linearizes action and layout, acknowledges complete projection;
Python/project/config/journal reconcile that identity all-or-none. Irreversible
claim without observed ACK keeps old/new/action pins, fences conflicts and records
uncertainty; do not claim rollback, success or early cleanup. Crash recovery uses
verified current revisions/material and fresh native ACK, never persisted tokens.
No36 independent unload loop. Callback applies only prepared bounded fixed handles;
capacity and drain budgets are future measured implementation proofs.

Move/Swap retains living identity/cohort/playhead/DSP/history and selected content/
editor follows. Copy leaves source/holds intact and creates fresh stopped content;
overwrite/clear stops/fences only actually removed targets. Global stop-restore,
active/paused/editor/telemetry projections must remap or retire with native identity.
Fixed MIDI pad trigger/stop/selection mappings stay at slots for future input.
Already admitted selected pitch events follow live lifetime or fence removed one.
Visual pressed state is no authority. Global middle-hold output mute remains global
and restores latest intended volume independently; no content-scoped mute abstraction.

HoldAction binds ContentInstance/lifetime/AcceptedActionId/native caused PauseEffect/
control revision/playback-cohort handles+generations, input origin and matching
physical waveform right press, original timing and diagnostic slot/epoch. Already
stopped/paused has no owned effect. Enqueue is admission, not effect ACK. Release
uses native immutable guards, never selection or a Python-resolved unguarded slot.
Matching button-up/cancel delivered by existing input/control tick outside hover/
selection/editor render/focus. Release-before-ACK retains same release-requested
record; ordered native pause/release settles effective/noeffect/retired. Saturation
retains exactly one bounded request/pins with visible pending/error and polling
retry. Duplicate releases consume once. Later STOP/retrigger/resume/intentional
pause/cohort replacement revokes old claim; continuity-proven same cohort may carry.
Move/Swap carries it; Delete/reassign/overwrite/all36 removed lifetimes fence it;
Copy no token. Release versus commit must prove both legal serial orders, delayed/
stale ACK, superseding intent, queue pressure and repeated mutations/ABA. Shutdown
settles stop/retirement before final pins; physical holds never persist on reopen.

All-bank assignments, immutable versions, old/current voices, reader/history/FIFO,
job subscribers, accepted actions and native unload ACK participate in final use.
Remaining interests prevent job cancellation; cancellation does not end actual read
leases. Removed action pins protect queued handles but cannot keep removed content
playable. Checked identity cleanup preserves external/private/unknown files.

Gesture owner branches before mouse trigger/stop even at drag start. Left empty
Move/full Swap; right Copy/full overwrite without single-pad dialog; clear target/
operation preview. Cancel/self/invalid/empty source no-op. Mode warning/help preferred
bottom-left, OFF after restart, MIDI alive. Other-bank pulse; left exact other bank
copies all36 including empties, occupied target OVERWRITE/CANCEL with bank name,
source remains selected/no navigation/selfcopy. Right any clicked bank CLEAR BANK/
CANCEL including current. Hover/dragend consumed before bank actions. Full requirements
and HC-01..HC-26 below remain future proof, with R2/R4 separate native closures and
P6/V0 repeats; hardware-free evidence cannot close real H-LIVE/H-FINAL.
