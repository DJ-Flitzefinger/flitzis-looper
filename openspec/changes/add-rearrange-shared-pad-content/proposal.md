# Rearrange independent content on fixed controller slots

## Why

The requested once-prepared/many-pad workflow requires Copy, Move, Swap and full
bank operations without duplicate files, origin dependence, voice restarts or late
slot actions affecting replacements. Today's slot-only waveform hold is not a
future movable-content release contract; successful unload already clears it.

## What changes

Introduce explicit stable PadSlotId/slot epoch, movable ContentInstanceId/lifetime,
shared immutable material/analysis/StemSet versions and current native source/
timing/window/action/effect authority. Copy musical intent into fresh stopped
content; carry living voices/history/holds on Move/Swap; fence removed lifetimes.
Use prepared bounded native pair/all36 transactions with journal/config recovery
and guarded ACK. Add Re-Arrange gestures/bank confirmations through existing input.

## Delivery and related contracts

R0 is official contract/docs revision only. [Program](../../../docs/pad-owned-pcm-program.md)
and [full coverage map](../../../docs/pre-rust-program-coverage.json) own unchanged
38 IDs/48 edges/all56+9/seven groups. J0 precedes P1b shared subscriber fanout.
P1a/b provide identity/leases, R1 CopySnapshot, R2 pair/native HoldRelease proof,
R3 pad gestures, R4 all36/native hold retirement proof, R5 bank UI; P6/V0 repeat
lifecycle/resources/pitch integration, H-LIVE/H-FINAL remain genuine human gates.
All HC-01..HC-26 remain future OPEN and need distinct slice closures. R0 needs
its own terminal then new separate native nonauthor closure before any J0/P1.

Revised `adopt-pad-owned-pcm-residency` owns canonical material paths, migration,
complete integrity, durable musical masks, finite/lazy216 and true last users.
This change owns content/placement/gestures/hold integration, not a competing store.
Existing baseline pad trigger/stop and Bank Selector are modified only for active
Re-Arrange; normal mouse/MIDI semantics remain. Current baseline specs stay delivered
behavior until effective deltas are implemented/merged; do not restore superseded
physical-duplication/session-mask targets when archiving.

## Non-goals and realtime constraints

No product code/runtime/build/install/device/GUI/listening/model operation in R0.
No independent stem pitch/retrigger engine, second scheduler/loader/cache, broad
input rewrite, plugin hosting or full Rust-app port/Slice8. Native callback only
validates fixed guards and commits prepared bounded handles with reserved ACK/
retirement; all tables/journal/file work, jobs, cleanup and heavy destruction stay
off callback. No I/O/JSON/Python/GIL/UI/locks/logging/inference/heavy allocation or
unbounded loops. Slot-array swaps/unload+reload do not prove live continuity.
