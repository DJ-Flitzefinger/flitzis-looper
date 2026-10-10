# Pad-owned audio, persistent stem PCM and deliberate residency

## Why

The existing hybrid app has complete FullMix PCM caches and finite dry loop views,
but originals, PCM and stems use separate global containers. Stem window relocation
still decodes and aligns complete PCM16 WAVs before cropping. Startup eagerly
publishes restored stems, and the UI cannot request an existing disk set whose
RAM is absent. Native active publication rejects first residency activation.

The revised target gives all occupied slots equal references to canonical immutable
material versions in `samples/<Originalfilename>/`, with #1..#216 identifying
stable slot membership rather than duplicate audio ownership. It retains reusable aligned stem PCM on disk, prepares actual required
source ranges, and loads stems deliberately. All 216 occupied resident slots are
a required case. Faster triggers and lower RAM require actual measured evidence.

## What changes

- Put byte-exact originals in each immutable material version's `original` area,
  complete FullMix/aligned stem PCM in `.pcm-cache`, and five WAVs in `stems`.
  Copy adds equal references without analysis, decoding or file duplication.
- Commit aligned complete stem playback PCM and five WAV artifacts per material, with a joint verified immutable set descriptor;
  read proved resident ranges from retained verified descriptors.
- Preserve FullMix until guarded native first-residency adoption and effective-mode
  acknowledgement; retain the existing bounded live crossfade and source continuity.
- Default startup and generation to FullMix-only residency. Keep saved ALL STEMS
  intent; explicit ALL STEMS requests preparation even when that intent is unchanged.
- Add a project-persisted startup stem preload setting, default off, on the existing
  Settings surface, with resource estimates and deliberate bounded RAM policy.
- Prove complete 216-slot admission, changed-pad-only refresh, Windows ownership,
  finite DSP coverage, migration/rollback and cold/warm resource/performance behavior.

P0 is the historical published plan. R0 revises its incompatible physical topology
after independent bounded P0 and complete C1 plan acceptance; R0's separate native
nonauthor closure and publication are required before J0/P1a/P1b. The extended
serial program in [tasks](tasks.md) delivers the feature. No intermediate layout, labelled
full-track fallback, deferred admission or document check completes that feature.

## Contract lineage

The direct seven-group extension supersedes only the incompatible per-pad
physical-duplication/no-shared-store contract and session-only musical-mask target.
All original integrity, migration, finite-DSP, lazy216, resource, performance and
human gates remain. The companion changes `add-key-transposition-performance` and
`add-rearrange-shared-pad-content` extend control/identity contracts. Existing base
specs describe delivered behavior until these pending deltas are implemented; archive
merge must use their effective targets, including durable masks.

This change builds on the delivered `cache-full-pcm-and-resident-loops` and
`bind-prepared-stem-publication` foundations. Their accepted evidence/tasks remain
historical. Its new PCM requirements add stronger end-state obligations; its
MODIFIED stem requirements replace the former global container and inactive-only
first-adoption wording **when implemented**, retaining independently guarded active new-generation adoption under E11-05/19. Project persistence explicitly replaces eager restored
publication. Performance UI explicitly replaces the disabled-unless-resident
mode control. Base specs and delivered-runtime docs remain distinguishable from
this pending target. Archive/merge must reconcile these effective deltas rather
than reinstate a predecessor's superseded wording.

## Non-goals and realtime constraints

No product code, model acquisition/inference, GUI/app/CPAL/recorder/device run in
P0 or R0. No separator reimplementation, new analyzer selector/default cutover, timing
evidence invention, automatic harmony or scale correction, new FX/plugin hosting or application
Rust-port planning/implementation/Slice8. Existing clickfix acceptance is separate.
Original B2-B8/K1/Slice7 and final human/device/hearing/correction gates stay OPEN.

All source capture, migration, hashing, JSON, PCM preparation and large retirement
stay on control/background paths. The callback consumes prebuilt immutable PCM
with bounded guards, acknowledgements, transitions and retirement capacity; no
disk I/O, Python/GIL/UI, blocking locks, logging, inference, plugin scanning,
unbounded loops or heavy allocation. [Design](design.md) and [tasks](tasks.md)
define the serial implementation and acceptance boundaries.

## Direct Human extension 2026-10-11 — PLAN_ONLY

The [bounded extension](../../../docs/pre-rust-extension-20261011.md) adds all E11-01..20 without reopening completed historical subtasks or accepting unimplemented behavior. It supersedes the opaque material root with `samples/<Originalfilename>/<Originalfilename>`, stems/ and .pcm-cache/; hidden full-content SHA identity and immutable generations remain. It supersedes total-extent 1-GiB rejection with bounded streaming conversion/full warm verification, preserving separate scratch/live/analysis limits. Offline generation during playback is separated from verified disk commit and independently guarded active adoption; existing runtime guards are unchanged by this plan. Companion changes extend-shared-stem-generation, refine-loop-editor-performance-actions and add-live-mix-recording own new stem/UI/recording behavior.

X11-KEYLOCK is the prioritized bounded production k=0 fixed NormalLoop correction built on accepted vertical and lifecycle proofs, not whole P4b/B5 acceptance. Full scope, original 38 IDs/48 edges, 169 rows, 56+9 and HC01..26 remain; additive DAG and actual gates are in the maintained program/coverage. No product implementation, runtime acceptance or full Rust-port planning is part of X11-PLAN.
