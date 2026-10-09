# Pad-owned audio, persistent stem PCM and deliberate residency

## Why

The existing hybrid app has complete FullMix PCM caches and finite dry loop views,
but originals, PCM and stems use separate global containers. Stem window relocation
still decodes and aligns complete PCM16 WAVs before cropping. Startup eagerly
publishes restored stems, and the UI cannot request an existing disk set whose
RAM is absent. Native active publication rejects first residency activation.

The requested final product owns each occupied pad's assets in `samples/#1` through
`samples/#216`, retains reusable aligned stem PCM on disk, prepares actual required
source ranges, and loads stems deliberately. All 216 occupied resident slots are
a required case. Faster triggers and lower RAM require actual measured evidence.

## What changes

- Put byte-exact originals, `.pcm-cache` and `stems` in each pad's visible folder,
  with transactional migration and immutable internal generations.
- Commit aligned complete stem playback PCM in each pad's `.pcm-cache` and five
  WAV artifacts in its `stems`, with a joint verified immutable set descriptor;
  read proved resident ranges from retained verified descriptors.
- Preserve FullMix until guarded native first-residency adoption and effective-mode
  acknowledgement; retain the existing bounded live crossfade and source continuity.
- Default startup and generation to FullMix-only residency. Keep saved ALL STEMS
  intent; explicit ALL STEMS requests preparation even when that intent is unchanged.
- Add a project-persisted startup stem preload setting, default off, on the existing
  Settings surface, with resource estimates and deliberate bounded RAM policy.
- Prove complete 216-slot admission, changed-pad-only refresh, Windows ownership,
  finite DSP coverage, migration/rollback and cold/warm resource/performance behavior.

P0 delivers only this official plan, deltas, maintained documentation and serial
handoff. P1-P6 below deliver the complete feature. No intermediate layout, labelled
full-track fallback, deferred admission or document check completes that feature.

## Contract lineage

This change builds on the delivered `cache-full-pcm-and-resident-loops` and
`bind-prepared-stem-publication` foundations. Their accepted evidence/tasks remain
historical. Its new PCM requirements add stronger end-state obligations; its
MODIFIED stem requirements replace the former global container and inactive-only
first-adoption wording **when implemented**, retaining inactive generation and
content replacement. Project persistence explicitly replaces eager restored
publication. Performance UI explicitly replaces the disabled-unless-resident
mode control. Base specs and delivered-runtime docs remain distinguishable from
this pending target. Archive/merge must reconcile these effective deltas rather
than reinstate a predecessor's superseded wording.

## Non-goals and realtime constraints

No product code, model acquisition/inference, GUI/app/CPAL/recorder/device run in
P0. No separator reimplementation, new analyzer selector/default cutover, timing
evidence invention, musical mask change, new FX/plugin hosting or application
Rust-port planning/implementation/Slice8. Existing clickfix acceptance is separate.
Original B2-B8/K1/Slice7 and final human/device/hearing/correction gates stay OPEN.

All source capture, migration, hashing, JSON, PCM preparation and large retirement
stay on control/background paths. The callback consumes prebuilt immutable PCM
with bounded guards, acknowledgements, transitions and retirement capacity; no
disk I/O, Python/GIL/UI, blocking locks, logging, inference, plugin scanning,
unbounded loops or heavy allocation. [Design](design.md) and [tasks](tasks.md)
define the serial implementation and acceptance boundaries.
