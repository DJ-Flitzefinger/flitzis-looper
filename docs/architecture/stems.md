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
paused, but new heavy inference waits for playback to stop under the resource
policy below. This lease grants no assignment, accepted timing, resident-window or live
publication authority. Worker completion and borrowed reader retirement are both
required before cleanup, including cancelled/obsolete results.

Heavy stem generation and analysis children share one resource permit on their
existing offline supervisors. The two-worker/32-queue stem pool and original
single-analysis admission remain job authorities. An admitted analysis waits
cancellably for heavy capacity; contention cannot count as a numerical failure
or select approximate BPM. Native KeyNet's local ORT session uses one intra-op
and one inter-op thread; its existing parallel analysis branch remains separate.
Child environments bound numerical pools before imports, separator children
verify effective Torch counts before model work, and FFmpeg decoder/encoder/filter
pools are explicitly bounded. App-global numerical settings are unchanged.

New inference displays **Waiting for playback to stop** while the existing active
PAD projection is nonempty. Saved-set lookup and component preparation retain
their separate lanes. If playback becomes active during generation, the existing
control poll requests an explicit playback-headroom failure; the user can retry
after playback stops. This never stops an active PAD. Observation and cooperative
abort/forced tree retirement have a finite tail: this is not zero-overlap or
device-deadline proof. A second admitted worker reports **Waiting for background
resources** rather than a completed execution device.

On Windows, before child activation an enforced Job commit ceiling is promised
on the shared Native host ledger. It deducts all outstanding Native promises and
retains the original 2 GiB commit / 512 MiB physical admission floors. The ceiling
uses half of current post-floor, post-promise headroom, with no new whole-track
size ceiling. The other half covers the deliberate actual/prospective double
charge, so a fully allocated child still leaves the original floor margin for
new resident/control work. It is a
resource budget, not a model-memory estimate or a reservation against unrelated
processes. Actual child commitment is conservatively double charged until the
tree and both output readers finish; conversion receives its own existing bounded
promise afterward. The original 3 GiB recovery and 512 MiB critical commit guards
remain. Recording finalization keeps its independent existing owner and does not
wait behind separator inference.

Windows enforcement does not establish Linux/macOS host monitoring or complete
POSIX descendant retirement. Those existing platform limits remain explicit final
distribution gates; child-only numerical limits and the portable core still apply.

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

Inference uses owned subprocesses. Numeric conversion now uses the independent
`flitzis-stem-conversion` Rust core through a thin synchronous detached binding.
Python retains positioned RIFF/RF64 readers, exclusive private writers, full
digest/identity verification and private-generation replacement. The core keeps
global binary64 positions, compensated mono averaging, ties-even PCM16 rounding
and ordered instrumental clipping before requantization. A single-use host
promise precedes even source EOF probes and covers raw spans, native/Python
payload overlap and writer/scratch space. Cancellation is checked around each
bounded block and before writes; native work returns before worker custody ends.
The validated native component identity binds the loaded converter's embedded
numeric/bridge/Cargo sources, lock and platform/profile. A supported complete v3
producer proof additionally composes this component with every guarded Python
role. Currently both complete inference-host manifests remain fail-closed UNKNOWN
at the unchanged unsupported Pydantic execution validator, also reproduced on
the parent; this port does not promote component proof to full producer proof.
Narrow validator/schema/dependency attestation remains a final regression gate.
This is compiled-source identity, not loaded-image or full compiler attestation.
Conversion does not remove Torch or prove model quality.

Committed component preparation
uses a cancellable worker-only gate, preserving serialized sharing and memory
peaks without retaining owners or the control producer during reads, allocations,
ledger commits or file-pin retention. Short owners sections revalidate the exact
proposal, source, content placement and intent before and after preparation;
the existing guarded publication and own effective ACK remain necessary.
The public request captures its original per-content admission order before
releasing its Engine borrow. Owned shared control contexts detach reconciliation,
binding capture, both registration paths and admission. Only successful admission
advances the independent explicit/policy watermark; registration has its own
successful-order watermark. Failed newer work preserves an earlier viable request.
Final registration rechecks source/window, placement, discard and the captured
lane's lifecycle after lock waits.

Source/window preparation uses the same worker-only gate before short owners
sections. Source and component bytes, owner witnesses, assignment reader locators
and source history are promised before source I/O. History reservations follow an
immutable accounting row identity through Move; Copy keeps independent target
history, and an ended row cannot authorize its new occupant. Worker-held historical
PCM remains charged even after its original view ends. Complete legacy conversion
has a separate prospective host reservation and retains its existing 1-GiB cap.
The worker revalidates the original content/lifetime, bank/window, proposal and
intent after I/O/registration, then releases its preparation gate before the
unchanged publication CAS and operation-specific ACK.

Coordinated child CPU/process/commit admission is delivered by STEP16. Remaining
provider selection and whole-process memory evidence are recorded in STEP17.
Controlled source/component/registration progress does not establish whole-GIL
isolation, device latency or the cause of callback glitches. The focused
[performance plan](../development.md#performance-audit-and-upgrade-plan) separates
control-lock isolation, detached Rust conversion and background resource policy.

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

The isolated [BS spectral Rust trial](../../rust/crates/stem-dsp/src/lib.rs)
implements pinned STFT2048/hop441, frequency-leading stereo features, four complex
masks and ISTFT with the original complete chunk length. It is not an application
route or full network replacement. Original Torch FFT-only comparisons do not
establish learned-mask, CUDA AMP, whole-track overlap, quality or distribution
parity; productive separators and dependency closures stay available. The
[STEP25 design](../../openspec/changes/improve-performance-isolation/design.md#step25-impact-independent-bs-spectral-frontend-and-reconstruction)
records its caller-owned resource/reader envelope and exact evidence limits.

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
