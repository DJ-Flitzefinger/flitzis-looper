# Stems

Stems are generated offline from a complete leased source. A verified disk set,
a saved selection, resident component data and audible selection are separate.
[Media and PCM](media-and-pcm.md) defines shared custody and last readers.

## Requests, workloads and models

[`stems.py`](../../src/flitzis_looper/controller/stems.py) captures the content,
source, selected model and processing settings for each request.
[`stem_identity.py`](../../src/flitzis_looper/controller/stem_identity.py) and
[`stem_provenance.py`](../../src/flitzis_looper/controller/stem_provenance.py)
resolve producer and source identity. Compatible known requests can join or
reuse a verified result. Each subscriber retains its own selection interest
and existing unknown producer provenance stays unknown. The
[`OfflineStemSourceLease`](../../rust/crates/looper/src/audio_engine/offline_stem_source.rs)
binds complete immutable material, actual source generation and target geometry.

Saved-set lookup can run while a target is playing or paused. A new inference
waits until current subscriber PADs no longer hold active or paused voices. If
a subscriber starts during generation, the poll path requests playback-headroom
failure and retires that attempt. Unrelated PADs may play while a stopped target
qualifies, subject to resource admission.
[`stem_workers.py`](../../src/flitzis_looper/controller/stem_workers.py) admits
two physical workers and 32 queued jobs. Stem inference and heavy pair
preparation share FIFO permission with full BPM/Key analysis. Ordinary Cold
playback preparation and recording finalization retain independent paths.

| Separator | Current inference path |
| --- | --- |
| BS-RoFormer MUSDB18HQ | Fresh default. The vendored Torch worker uses the pinned v1.0.12 config/checkpoint. Audio is stereo at 44.1 kHz with 485100-frame chunks, overlap 2 and batch up to 2. CPU uses FP32 and CUDA uses AMP. Disk-backed input and rolling overlap-add bound PCM staging. Short final chunks use reflection/zero padding. |
| Demucs `htdemucs` | Supported saved/user choice. The locked Torch CLI retains whole-track model input/output and the configured shifts/overlap. |
| Native HTDemucs CPU | Explicit Windows x64 trial route through the gated adapter. The [release and worker slots](../../workers/htdemucs/native_cpu/) carry no approvals in this checkout, so that selection is unavailable. |

[`stem_separators.py`](../../src/flitzis_looper/controller/stem_separators.py)
dispatches the selected producer. A recognized CUDA failure can make one fresh
CPU attempt for the same model. Integrity, codec, cancellation and resource
failures retain their own outcomes. The
[`ExecutionReceipt`](../../src/flitzis_looper/stem_execution_protocol.py)
records the actual device, including fallback reason. Hardware availability
and completed model loading establish readiness. The
[setup guide](../model-worker-setup.md#optional-stem-models-and-codecs) covers pins,
FFmpeg and offline provisioning.

Child numerics and FFmpeg pools are bounded. On Windows admission observes
actual host capacity and low-memory notification. The suspended child joins
an owned Windows Job before execution. Linux uses the owned subtree broker.
Platforms without an owning adapter refuse new subprocess work. Terminal
status can precede process/tree/pipe or lease retirement, so permissions and
scratch ownership stay attached through actual settlement.

## Complete artifacts and assignment

The complete outputs are drums, bass, other, vocals and the instrumental
non-vocal sum. [`stem_artifacts.py`](../../src/flitzis_looper/controller/stem_artifacts.py)
uses the detached [Rust conversion core](../../rust/crates/stem-conversion/)
for bounded resampling, ordered summation and requantization. Python owns
positioned PCM16 RIFF/RF64 reads, exclusive writes and generation custody.
The stem pair binds five WAV outputs and complete aligned float32 derivatives.
Native [streamed alignment](../../rust/crates/looper/src/audio_engine/stem_cache/streamed.rs)
uses complete onset scanning, a four-second alignment window with a half-second
pre-onset, and a ±0.25 second lag search with ascending tie order. Hashes,
geometry, EOF and source binding are checked through sealed readers.

[`stem_cache.py`](../../src/flitzis_looper/controller/stem_cache.py) resolves
material-root `stems` and `.pcm-cache` with full identity checks. A joint
commit and fenced configuration point to an immutable complete set. Verified
warm restoration can recover a saved set without model inference. Cross-rate
output restore rebuilds the aligned selection as a schema-2 Pair from complete
Original-clock evidence with fresh ownership. The exact schema-1 read and
alignment policy remains available for its original rate.

## Residency, switching and final readers

| State | Authority |
| --- | --- |
| Complete disk set | Verified original/source and output identities, independent of saved selection. |
| Saved selection | Exact persisted set; batch Done reports this boundary. |
| Pending candidate | Captured content, source/window, voice/launch, timing and selection request. New-set registration is separately guarded. |
| Resident | Budgeted required component PCM and reader/history coverage. |
| Effective | Matching callback adoption and the operation's acknowledgement; actual wet output may still wait for resume. |

[`committed_stem_residency.py`](../../src/flitzis_looper/controller/committed_stem_residency.py)
projects the states. [Native preparation](../../rust/crates/looper/src/audio_engine/committed_stem_residency.rs)
reserves source, window and history outside the callback. The
[mixer](../../rust/crates/looper/src/audio_engine/committed_stem_mixer.rs)
revalidates ownership, permits and capacity before the 128-source-frame
transition on the existing cursor. Pause stops transition progress.
Running/paused, Intro/Normal/Tail, finite/complete and Key Lock paths need
their admitted reader and history domains. FULL MIX with preload off can
release unused components after the effect and outgoing final ramp while
keeping the disk selection.

[`stem_batch.py`](../../src/flitzis_looper/controller/stem_batch.py) submits
loaded PADs across banks within queue limits. Cancelling one subscriber leaves
shared work for others intact. Delete revokes captured assignment eligibility
before physical cleanup. Queued work, current and outgoing voices, source
leases, workers and prepared history can still hold readers. The
[Native asset owner](../../rust/crates/looper/src/audio_engine/project_assets.rs)
checks last use, file identity and custody off realtime.

The [stem-cache contract](../../openspec/specs/stem-cache/spec.md) specifies
selection and retirement. The [archived performance design](../../openspec/changes/archive/2026-11-13-improve-performance-isolation/design.md)
preserves scoped measurements and incomplete provider/producer evidence.
