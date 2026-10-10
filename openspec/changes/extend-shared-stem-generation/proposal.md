# Playback-independent shared stem generation and all-loaded actions

## Why

The Human extension of 2026-10-11 requires safe offline stem generation during
playback, Generate/Delete actions for every loaded pad, one current model's set
and duplicate prevention (E11-05/06/07/19/20). Published source already isolates
private generations, shares job subscribers and limits the pool to two workers
and 32 queued jobs. It still rejects playing pads in both controller completion
and native new-set adoption, lacks a durable model/configuration fingerprint and
has no all-bank batch interface. Same-set P5a residency is a distinct obligation;
it does not prove adopting newly generated content.

## What changes

- Separate immutable source-leased offline jobs and fully verified WAV/PCM/disk
  commits from each pad's independently guarded native preparation/adoption.
- Permit generation during playback after the new safety gates pass; preserve
  current FullMix/selected stems, Native/FIFO/filter chronology and transport
  while preparation, cancellation or rejected adoption settles.
- Keep one current verified set per shared material, binding its model and
  effective configuration fingerprint. Replace the current selection atomically
  while retaining immutable predecessor files for their actual readers.
- Disable Generate for a valid current selected-model set and enforce the same
  duplicate rule in direct, controller and batch requests.
- Add warning-confirmed all-loaded Generate/Delete actions over the complete
  216-slot, all-bank inventory, bounded fair job admission, progress, partial
  results, visible errors and batch-interest cancellation.

## Contract lineage

These are pending targets, not claims that published runtime guards are already
open. On implementation/merge, the new offline-generation and new-set-adoption
requirements supersede the activity-only restrictions in `stem-cache`'s
`Stem Generation And Replacement Require An Inactive Pad`, `background-tasks`'s
`Stem Tasks Respect Per-Pad Concurrency`, `Stem Task Completion Is Revalidated
Before Publication` and `Performer Stem Generation Uses Background Tasks`, and
the inactive-only clause of `integrate-bs-roformer-musdb18hq`'s `Separators Share
Bounded Publication`. Reconcile the corresponding active deltas in
`cache-full-pcm-and-resident-loops` and `adopt-pad-owned-pcm-residency`; the
`Same-source stem windows` relocation contract still identifies the identical
accepted set and cannot be repurposed as new-generation authority.

The replacement removes only pad activity as an offline-generation blocker and
adds a separately proved new-set adoption path. Current source/request, timing,
window, voice/history, capacity, integrity and own-ACK guards remain. Historical
completed tasks and negative evidence remain unchanged. The existing native
publication API must keep rejecting active new sets until that path is proved.
Readable roots/content identity and long-source streaming are delivered by the
companion X11-MATERIAL and X11-LONG contracts; this change uses them rather than
duplicating their resolvers or changing their scope.

## Non-goals and realtime constraints

X11-PLAN only adds contracts and delivery gates. It runs no model inference,
product implementation or App/GUI/CPAL/device/listening acceptance. No new
separator, multi-model library, analyzer change, source replacement during a
voice, scheduler, continuous reader framework, plugin host or full Rust-port
planning/implementation is included. Whole P4b/P5a/P6 and final Human gates
remain open until their own genuine results.

The callback mixes prepared available PCM only. Model loading/inference,
hashing, JSON, disk operations, Python/GIL/UI calls, blocking locks, logging,
unbounded work and heavy allocation remain outside realtime processing. Bounded
prebuilt transactions, scalar/identity checks, transitions and retirement lanes
must reuse the existing architecture. See [design](design.md) and
[tasks](tasks.md) for dependencies and genuine acceptance.
