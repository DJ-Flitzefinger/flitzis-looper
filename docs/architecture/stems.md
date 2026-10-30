# Stems

Stem generation is offline work over immutable complete material. Disk selection,
RAM residency and effective audio adoption are separate states. Shared immutable
data does not share a PAD's mutable playback or acknowledgement. Common source,
custody and retirement rules are in [media and PCM](media-and-pcm.md), timing in
[runtime](runtime-and-transport.md), pitch/history in [Key Lock](pitch-and-key-lock.md).

## Request, identity and shared work

[controller/stems.py](../../src/flitzis_looper/controller/stems.py) orchestrates
Generate/Delete, selection and current feedback. GUI/MIDI/controller actions
capture the current content/source and selected separator; changing Settings
affects future requests. [stem_identity.py](../../src/flitzis_looper/controller/stem_identity.py)
and [stem_provenance.py](../../src/flitzis_looper/controller/stem_provenance.py)
resolve the complete known producer fingerprint before duplicate admission.
Compatible known requests can join one inference or reuse its verified result.
Render-time UI does not hash models. Settings/material ABA fences stop old work
selecting newer content. Historical UNKNOWN lineage stays UNKNOWN; a verified
model-free restore does not prove its producer identity. Known producer lineage
requires actual execution/birth witnesses and bounded file custody (64 files),
not just source hash, settings or the currently installed model. Historical
37-file Demucs/33-file BS observations are scoped evidence rather than a claim
that all possible inputs/process trees have certified lineage.

[stem_workers.py](../../src/flitzis_looper/controller/stem_workers.py) owns two
workers and 32 queued physical jobs, reserving capacity before ownership changes.
[stem_job.py](../../src/flitzis_looper/controller/stem_job.py) retains source and
private-generation leases until the backend returns and its queued result settles.
Each assignment/subscriber has its own interest. Cancellation/Unload removes that
interest; it cannot erase another subscriber's job or active source readers.

[offline_stem_source.rs](../../rust/crates/looper/src/audio_engine/offline_stem_source.rs)
captures an `OfflineStemSourceLease` for the actual immutable complete source,
cache marker/generation and target geometry. Generation may run while playing or
paused. This lease grants no assignment, accepted timing, resident-window or live
publication authority. Worker completion and borrowed reader retirement are both
required before cleanup, including cancelled/obsolete results.

## Complete artifacts and durable selection

Source-backed paths are material-root `stems` and aligned `.pcm-cache` areas,
resolved by [stem_cache.py](../../src/flitzis_looper/controller/stem_cache.py). The implemented
legacy slot-container fallback is not the sole productive storage model.
Full source/root identity is checked; a readable folder or shortened ID is not
enough. Unknown files, replaced paths and reparse routes do not become owned.

Private generation becomes an immutable verified ready set. Native
[stem_pair.rs](../../rust/crates/looper/src/audio_engine/stem_pair.rs) prepares and
opens the complete pair, binding source/processing geometry and digests for five
WAV outputs and their aligned f32 derivatives. Instrumental is the established
pre-quantization non-vocal sum. Complete decoding/alignment/export uses bounded
blocks and supports long geometry separately from inference/admission limits.

[stem_cache/streamed.rs](../../rust/crates/looper/src/audio_engine/stem_cache/streamed.rs)
scans complete source/components for global onset maxima, then the first 5-percent
threshold crossing. Alignment retains the four-second window, 0.5-second pre-onset,
plus/minus 0.25-second lag and ascending tie rule. PCM16 MIN maps to -1, other
values divide by 32767; fixed f32 component order and signed offsets preserve exact
padding/tail. Descriptor-last eligibility and fresh reopening verify all hashes,
source/mono binding, geometry and EOF through sealed readers.
[pcm16_wav.py](../../src/flitzis_looper/pcm16_wav.py) supports positioned RIFF/RF64
PCM16 reads and exclusive writes; large output uses ds64 counts.
[stem_artifacts.py](../../src/flitzis_looper/controller/stem_artifacts.py) streams model-output
resampling/instrumental summation with the established small-file rounding oracle.

The joint set commit covers both disk areas; fenced config selection points to
the exact immutable result. It does not force unused components into RAM or
certify a live effect. FULL MIX with preload off need not retain unused stem
windows. Warm restoration verifies complete source/artifact integrity rather than
trusting directory existence. Model-free restoration can reuse a verified set
without rerunning its separator. A changed selected model does not invalidate an
unrelated already selected source-bound set.

## Disk, resident and effective state

| State | Owner and proof |
| --- | --- |
| Generated/verified disk pair | Complete immutable source/output identity; registration alone does not adopt it. |
| Saved selection | Fenced ProjectState/config selects an exact set; batch Done means saved. |
| Pending candidate | Own current content/source/timing/window/voice/history and selection request, with separate new-set registration. |
| Resident components | Budgeted actual component windows; data availability alone grants no effect. |
| Effective mode/mask/set | Matching callback adoption and own acknowledgement, with actual output/ramp state where needed. |

[committed_stem_residency.py](../../src/flitzis_looper/controller/committed_stem_residency.py)
tracks registration, residency and effect independently. `register_committed_stem_pair`
seals disk data; `register_pending_committed_stem_pair` creates a separately guarded
new-set candidate. [prepared_stems_work.rs](../../rust/crates/looper/src/audio_engine/prepared_stems_work.rs)
retains the generic inactive new-source/new-set guard. Offline generation during
playback does not remove it or grant unrestricted direct active publication.

[committed_stem_residency.rs](../../rust/crates/looper/src/audio_engine/committed_stem_residency.rs)
captures actual source, voice, window, accepted timing, launch and history. Workers
reserve/load required components. [committed_stem_mixer.rs](../../rust/crates/looper/src/audio_engine/committed_stem_mixer.rs)
rechecks binding/content/launch/Native permit/candidate and feedback/retirement
capacity before adoption. Unsupported context, stale source, queue pressure or
missed preparation deadline refuses without borrowing another set/PAD's ACK.

The bounded transition is 128 source frames, following the same cursor, scalar
rate, EQ and Native path. Pause stops transition progress. Residency acceptance
can remain waiting for effective output until the unpaused current wet source
actually produces it. Normal/Intro/Tail, finite/complete, running/paused and ON/OFF
states need their own valid physical reader and history domains; saved ALL or
shared backing cannot substitute. Earlier paused-Intro rejection and later scoped
software support remain distinguishable evidence, not a universal live-domain claim.

Same-set preload is a policy interest. FULL MIX can release components after its
own effect ACK and outgoing final ramp, retaining the verified disk selection.
A mode/selection receipt can become historical after ordinary Play without
revoking an already admitted physical data stamp. UI separates saved replacement,
pending switch and last-confirmed audio; newer metadata cannot erase an old reader.

## Cancellation, batch work and final readers

[stem_batch.py](../../src/flitzis_looper/controller/stem_batch.py) feeds loaded
PADs across banks fairly within worker/queue limits. Shared results keep separate
interests. Batch cancellation is cooperative and does not cancel independent work.
Deletion confirms captured assignments, revokes eligibility first and separates
logical detachment from physical terminal cleanup. Done/saved is not audio ACK,
whole-216 resource acceptance or proof all files have disappeared.

Readers include source leases, workers/results, queued adoption, current/outgoing
voices, resident PCM and prepared Native/history. Cancelling a candidate revokes
future adoption without prematurely releasing those actual readers.
[project_assets.rs](../../rust/crates/looper/src/audio_engine/project_assets.rs),
[stem_readers.rs](../../rust/crates/looper/src/audio_engine/project_assets/stem_readers.rs) and the
shared retirement gate recheck file/hash/identity and actual last use. Cleanup is
off realtime; Unload, ACK, silence or a timeout alone cannot authorize deletion.
Close continues settlement; unknown/changed custody is preserved.

## Models, setup and actual execution

[Model setup](../stem-generation-setup.md) is the stable installation reference.
Fresh separator choice is BS-RoFormer MUSDB18HQ; saved Demucs choices remain.
Generate/Settings never downloads. Explicit setup resumes retained transfers and
checks pinned complete size/hash before atomic installation. Portable remains
self-contained/relocatable, independent of private developer cache policy.

[bs_roformer_assets.py](../../src/flitzis_looper/controller/bs_roformer_assets.py) pins release
v1.0.12/commit `aef04b2e52fb3beaf25e333199f5a7236e628e7b`, exact YAML/checkpoint
identities and pre-deserialization checks. The vendored worker hashes and loads
from one retained handle with `weights_only=True` and strict state matching.
The four stereo model outputs are drums/bass/other/vocals at 44.1 kHz; productive
inference uses 485100-frame chunks, overlap 2, batch at most 2, CUDA AMP or CPU FP32.
A rolling overlap-add ring and independent per-chunk windows bound PCM storage;
short final chunks follow the established reflection/zero-padding policy.
Nonfinite output fails. Exact upstream overlap-add numerical parity is not claimed.
The 183367800-byte PCM estimate excludes weights, activations, decoder internals
and process RSS. Demucs whole-track inference memory is not bounded by artifact
streaming or BS-RoFormer's PCM estimate.

CUDA failure may retry the same chosen model once on CPU with an execution receipt
and reason. It never silently changes BS-RoFormer to Demucs. Completed inference
device stays visible through later alignment; old missing receipts show unknown.
Model loading, CUDA availability and source/unit propagation checks are distinct
from actual separation. The historical E-CUDA delivery records actual paired
Beat This and Demucs CPU/CUDA executions, while its BS-RoFormer device propagation
was source/unit-only. Later CPU model-load readiness is not a BS separation receipt.

## Evidence and limits

The [canonical E-STEM/E-PCM/E-CUDA index](../../openspec/changes/archive/2026-10-30-evidence-index.md)
retains exact full reports, UNKNOWN producer/device lineage, queue/refusal/stale/
deadline/retirement failures, supported domain histories and remaining installed
inference/resource/power-loss/device/Human gates. RF64 source/parser support does
not establish five-RF64 separation. Fault injection is not actual disk exhaustion
or power-loss acceptance. Internal 32 ownership checks are separate from the
2/4/6 productive target and synthetic stress ceiling of 10 PADs.

Current [stem-cache](../../openspec/specs/stem-cache/spec.md),
[source alignment](../../openspec/specs/loop-source-stem-alignment/spec.md) and
[background tasks](../../openspec/specs/background-tasks/spec.md) contracts retain
these separations. This documentation changes no behavior or acceptance.
