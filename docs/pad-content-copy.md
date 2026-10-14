# Independent pad content copies

The empty-target Copy core copies musical content to a fresh stopped pad. It
shares verified immutable material, source PCM and matching stem backing, while
each assignment owns independent source custody, playback and mutable settings.
Occupied-target overwrite, live Move/Swap, bank operations and Re-Arrange gestures
have separate contracts in
[add-rearrange-shared-pad-content](../openspec/changes/add-rearrange-shared-pad-content/specs/pad-content-arrangement/spec.md).
This core does not close those later gates.

## Internal occupied-target foundation — 2026-10-14

The existing Native Copy transaction can now prepare an occupied target when
`capture_pad_copy` receives that target's exact opaque `target_binding`. Omitting
it still requires an empty target. Captured lifetime, source, timing, physical
placement, preparation, runtime, authority and bounded selection/proposal guards
must remain current. Equal PCM or an overwrite flag cannot authorize replacement.

One existing callback claim retires the original target's Hold effect and voices,
then installs a fresh stopped content. The source keeps its living voice, history,
DSP, cursor and holds. Displaced bank/stem/receipt handles and voice readers leave
through the existing off-realtime retirement lane. Fixed guards and conservative
retirement capacity bound callback work. Pending or pressure retains the exact
payload; a stale target or preclaim cancellation rejects without replacing either
participant. The own Native acknowledgement controls replacement of the target's
control cache, including removal of an old selected set when the source is FullMix
without selected stems.

The internal `PadArrangementController.copy_overwrite` captures the original
musical target seal and fences target observers until settlement. Only after the
own complete Native acknowledgement does it retire original controller tickets
and that target's job subscriber. Other subscribers and actual readers survive.
The occupied config path retains one prepared `AssignmentPublicationPlan` through
interrupted table transfer and old-owner retirement; retries settle its original
cursor before incorporating newer unrelated intent. Generic cancellation after an
actual disk-selection replacement does not revoke the committed selection.

A Pair-delayed duplicate generation retains its original source/model/subscriber
owners. It may reuse a ready completed set, transfer to an actually active canonical
owner under both job locks, or rejoin the existing queue after that owner settles.
An already captured terminal delivery cannot absorb another subscriber. Cancelled
or obsolete running readers retain their real custody while newer intent waits in
the existing material-serialized queue.

Occupied Copy now retains its own strict `pad_copy_overwrite` journal, immutable
A/B musical seals and one fixed fresh C through preparation, own Native ACK,
actual Config receipt and terminal settlement. Independent A/B/C original and
stem readers survive queued old-target cleanup and uncertain append retries.
Restart distinguishes exact original-input replay from a recognized written C:
replay needs the original A/B; recognized C obtains fresh target-only Native
acceptance while surviving-source changes remain intact. Saved selected stems
must acquire their own current registration and demanded component/preload ACKs
through the existing verification path before settlement. Complete history,
lineage or custody conflicts retain visible fences. See
[startup ownership](architecture.md#persistence-and-restore) for the controller
sequence and [Portable validation](portable-windows.md) for kind routing.

This is an internal software foundation. Public Re-Arrange gestures, complete
selection/Hold matrices and whole R2 acceptance remain open, as do musical,
resource, device and Human gates. It does not expose the pending
Key/transposition performance milestone.

## Durable intent and the snapshot

`ProjectState.pad_stem_intent` is the single musical stem-selection authority for
all 216 slots. Its versioned immutable value contains the enabled mask, remembered
custom mask and custom/instrumental/all display mode. Mask zero is valid. FullMix,
Delete Stems and unavailable components retain this intent; a completed new source
assignment or actual unload resets it. Legacy projects receive independent
defaults. Invalid rows recover visibly without replacing the original malformed
config; valid neighboring rows survive, and Copy rejects a known config fence
before obtaining a Native reservation.

An ordinary launch can invalidate the output observation of a previously accepted
stem receipt. Polling withdraws that observation and its matching historical retry
interest without changing the saved ALL/mask choice or releasing current audio
owners. A fresh explicit selection obtains its own receipt; independent preload
interest and actual Native failures retain their existing control paths.

`CopySnapshot` seals canonical validated JSON rather than retaining mutable model
children. It includes source/material birth metadata, analysis and selected pair,
timing/manual/TAP, loop/grid, Key Lock inheritance, key correction/base/extra/
retrigger, Gain/EQ and stem intent. Every projection revalidates and creates fresh
models. The target gets a fresh content UUID and historical `copied_from` lineage.
That lineage never grants permission to look up an originating slot.

Current source/timing/selection permits, runtime lifetimes, voices, DSP history,
cursors, hold effects, meters, pressed state and worker handles are excluded.
Persisted analysis evidence remains historical; only a current Native owner and
its own acknowledgement can make it eligible for playback or verified export.
Cold assignment and Copy allocate checked runtime lifetimes outside realtime.
An explicit replacement, empty slot or different complete source digest gets a
fresh lifetime; same-material window/timing maintenance retains its content's
lifetime. Current bindings and queued cold messages check that lifetime as well
as existing source/epoch authority. Rejected pending work restores only its exact
predecessor, and a genuine unload clears the active lifetime. Runtime numbers
are scoped by the receiving Native ownership instance and never persisted as
permission.

## One complete acceptance and durable publication

`PadArrangementController` owns one Copy at a time:

1. Capture the musical seal and fresh identity, acquire the existing config writer
   and retirement reservations, then reserve the actual empty Native target.
2. Prepare independent source custody, suitable shared PCM/views, selected pair and
   any accepted timing rebind on the existing bounded cold-worker lane.
3. Reserve assignment images before enqueueing one complete Native Copy payload.
   Preparation and enqueueing do not advance current target epochs.
4. The callback validates capacity and current guards, claims once, installs the
   complete stopped source/timing/stem/mask/Gain/EQ/key image and acknowledges last.
5. Reconcile that exact target receipt, merge the snapshot into the latest project,
   atomically save config and publish the new assignment custody.
6. Project target-only UI state and its own accepted stem-selection ticket, then
   release the Native hold and config writer.

The callback moves prepared handles and fixed scalar state. File/JSON/Python work,
verification, preparation and destruction of residual owners stay off realtime.
A distinct Copy parameter epoch prevents older Gain/EQ and legacy timing messages
from rewriting the installed image when the two message queues drain in different
orders. Target gestures and conflicting inheritance/tempo-anchor changes reject
before intent or queue mutation while settlement remains pending.

Preclaim failure can cancel without modifying occupancy or current epochs. A
claim without an observed complete ACK keeps owners and fences; an ordinary source
ACK cannot certify Copy. After actual adoption, config or callback failure retries
under the same retained transaction rather than guessing rollback. Unrelated newer
project edits survive the candidate merge and retry.

Source and selected-pair assignments use independent verified subscriptions.
Shared PCM is charged once per resident account, while every actual owner view
and physical reader remains represented in existing capacity accounting. Removing
the originating slot therefore cannot retire a surviving copy's material. The
last assignment, voice, queued work, retained ticket and physical reader still
participate in final reclamation.

Warm restore also reuses equal finite ranges. Complete original/cache validation
and held FileID checks run before consulting bounded weak range locators on the
existing cache. Equal ranges retain the same live PCM and immutable source
descriptor; each restored pad still builds and admits its own view and obtains
its own Native acceptance. The existing opening/preparation gates serialize equal
cache misses. Locator locks are released before resident accounting, and dead
views are pruned without retaining PCM or file ownership. Different ranges and
different verified interpretations cannot reuse this backing.
Concurrent recovery can briefly hold an unused warm staging marker. That exact
identity-bound directory then transfers to the existing reserved cleanup queue;
the verified cache remains usable and unrelated generations remain protected.

## Closing and verification boundaries

Close requests stop Copy admission and try genuine preclaim cancellation while
keeping controller custody until the actual preparation worker returns. A claimed
Copy completes its normal own-ACK/config/callback path before dependent controllers
or audio owners shut down. The UI keeps close pending and displays settlement
errors, polling without blocking the frame. The shell gates `app_shall_exit` in
`after_swap`, which the
[pinned HelloImGui loop](https://github.com/pthom/hello_imgui/blob/902dd7d8229b0f396a7f5d4e0a61486825b69c4a/src/hello_imgui/internal/backend_impls/abstract_runner.cpp#L1162)
runs before the next exit check. `before_exit` performs only permitted teardown.

The focused control tests distinguish scripted adapters from actual Native proof.
Native public tests use generated owned WAVs, real canonical import, real worker
preparation and the actual callback without opening a device. They exercise
targets 36 and 215, shared backing and independent custody, source continuity,
negative admission/queue/cancel/stale/claimed cases and productive config/assets
publication. Separate saved-QM and new-process tests cover their corresponding
evidence and lifetime boundaries. These software cases do not certify musical
quality, device behavior, listening, the full 216-pad resource matrix or later
Re-Arrange/HoldRelease integration.

The R1 software gate passes with these actual Native proofs, the impacted control
regressions, strict affected OpenSpec validation and an independent source review.
A private optimized Release passes API/Portable/import checks without installation;
537 focused model/control/UI tests also pass against that private Native module.
The earlier broad Native failures and their individual repairs remain recorded in
the local test log; this is not a claim of a fresh whole-workspace test run.
