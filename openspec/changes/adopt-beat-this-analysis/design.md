## Status and selected target

B1a's diagnostic boundary and B1b's optional local reference worker are implemented.
The target for future new analyses is `beat-this==1.1.0`, checkpoint `final0`,
minimal postprocessing (`dbn=False`). `small0` may be evaluated as an explicitly selected
alternative; it is never a silent fallback. A failed gate postpones cutover with recorded
remediation; it does not reopen model choice automatically. Corrected qm-dsp output remains
comparison/legacy evidence, not an implicit substitute when the selected worker is unavailable.

The `begin_offline_analysis`/`OfflineAnalysisService` boundary pins immutable loaded
`SampleBuffer.samples: Arc<[f32]>` with actual rate and source/request generation, streams shared
mono to a complete float32-LE file and releases that analysis pin before key preparation.
Both branches read the same loaded-rate mono without another decode. It leaves existing automatic
and manual `analyze_sample` routing intact. JSON diagnostic envelopes are not adopted into saved
analysis or manual grids. B1b adds explicit setup, an accepted final0 checksum, a separate
locked Windows CPU environment and the pinned 22050-Hz reference frontend. See
[setup](../../../docs/beat-this-setup.md) and [evidence](../../../docs/beat-this-reference-evidence.md).

## Input domains and preprocessing

Contracts implemented by the B1a records, plus explicitly deferred model/map layers:

| Record | Contract |
| --- | --- |
| Loaded PCM snapshot | Interleaved f32, loaded rate u32, channel/frame counts, source identity and generation, pad/request ID. This is the loaded playback-buffer domain, not compressed-file samples or output-device position. |
| Shared mono input | One complete loaded-rate f32-LE file streamed from that immutable snapshot; arithmetic channel mean accumulated in f64 and rounded to f32 once per frame. Same time-zero and duration, no silence trim or per-chunk time reset. |
| Beat worker input | Complete loaded-rate mono PCM plus metadata in `MonoPcmInput`/`BeatWorkerRequest`. B1b derives 22,050-Hz mono directly with pinned soxr HQ and centered log-mel preprocessing, preserving reference origin/rounded tail. |
| Key input | Read the staged mono through a retained native file handle in bounded chunks; derive a complete 44,100-Hz vector with the existing Rubato configuration for unchanged Rust CQT/KeyNet. No complete loaded-rate key copy or conversion through beat input. |
| Raw predictions | Beat/downbeat logits and detected f64 seconds with model/configuration/preprocessing identity. Nominal model frame k means k*441/22050 seconds; verify origin against preprocessing fixtures. |
| Accepted map | Separate reviewed anchors, beat units/counts, coverage and corrections under the versioned-map change. Loaded-frame positions are derived from seconds times the active loaded rate. |

One decode is retained. The old single common 44.1-kHz resampling requirement is intentionally
replaced by one common mono source and two required-rate branches. Manual analysis may prepare
or reuse derived analysis inputs; it must not re-run file decode, playback resampling/channel
mapping or sample publication. No path reconstructs audio from waveform min/max buckets.

Verify first/last impulses, silence, stereo conversion, fractional positions and complete tails
at 22.05/44.1/48/96 kHz. Model preprocessing and any resampler delay are accounted once. Retaining
f64 seconds preserves coordinate precision, not stronger detector accuracy. Cache provenance
includes loaded input rate and preprocessing identity because rate-dependent PCM can alter
inference even when the musical source is unchanged.

## Worker and job ownership

The boundary admits one native request per engine, one request per service and one process globally,
with zero pending queues. Native PCM admission is capped at 512 MiB for the maximum simultaneous
ownership in either export or key-preparation stage, as detailed in B2b2 below. The complete mono
file has an independent 512-MiB limit. This excludes FFT scratch, CQT/ORT workspace and worker
model memory. The adapter also bounds request/response/checkpoint sizes and prediction counts. Its provisional process timeout
is 120 seconds, followed by finite 5-second reap/reader-join attempts. Failed reaping retains
the process slot and PCM until actual retirement. Key CQT/ORT calls remain non-preemptible.
See [boundary reference](../../../docs/offline-analysis.md) for the complete implemented limits.
Those limits and test doubles are not real-model performance/quality acceptance.

Extend the existing analysis request lifecycle with a narrow beat-analysis adapter; do not put
process supervision in the large UI controller or add a competing scheduler. Rust control/
background code pins the immutable source and prepares a temporary shared PCM file in
bounded chunks. A small versioned request carries path/handle, exact dtype/endianness, rate,
frames, source/request IDs and selected model identity. Validate lengths/ranges before mapping.
Do not serialize full PCM as JSON or acquire Python objects from the callback.

Use a lazily started isolated local worker with a separately locked compatible interpreter and
dependency set. Windows launches it without a console window. Initial policy is one inference
job at a time, bounded pending requests, explicit input/output byte limits and bounded worker
thread count; measure and freeze limits in B1b. KeyNet stays on its Rust background thread and
can finish while the beat worker starts or is unavailable. No arithmetic promise that total
wall time equals the slower kernel ignores setup, IPC, queueing or publication overhead.

Progress and stale-result rejection reuse existing tracking. Current rejection after analysis
completion does not stop its computation; bounded cancellation, termination and resource cleanup
are new B1a responsibilities. Cancellation checks occur before export, between bounded
export/inference chunks and before publication; a timeout terminates an unresponsive worker
off-thread after a configured deadline. Do not report completed cancellation while the worker
still holds the cancelled job's PCM or computation resources. Unload/source replacement invalidates
the request immediately even if a model call
cannot stop instantly. Completed responses must match request, source, generation and model
identity; validate finite sorted times, counts and source extent off-thread. Atomically publish
one validated component-result envelope and retire temporary buffers/files off-thread after
readers finish. Resource limits reject oversize requests explicitly rather than truncating audio.

That termination deadline is a beat-worker contract, not a claim that Rust KeyNet/ORT can be
preempted. Cancel queued key work and add cooperative checks between controllable stages; an
already-running native inference call may continue. Track it as retiring until it actually
returns, reject its stale output and bound concurrent/retiring key slots plus retained PCM bytes.
Use backpressure when slots remain occupied; never spawn replacement threads without a limit.
The shared file and any in-flight key PCM remain owned until their readers finish. Whole-request
cancellation is not complete merely because the beat process exited: both branches must actually be terminal.
No hard whole-request cancellation latency is promised for a noninterruptible key call. Test a
stalled key double, unload/reload and shutdown reporting/resource ownership without blocking UI
or treating live key resources as released. A later hard deadline would require its own key
isolation/cancellation design, rather than forcefully killing a Rust thread.

## Independent outcomes and compatibility

Represent beat and key attempt states separately: ready, unavailable, failed or cancelled,
with provenance and any retained prior beat result identified explicitly. Valid key output may
publish even when Beat This is missing; valid beats survive key failure, with `unknown` for
the failed key result. A failure before usable preprocessing retains the previous complete
analysis. Never fabricate a positive BPM, grid or successful beat status for an unavailable
model. BPM remains a versioned summary of accepted beat intervals, not the timing authority;
freeze its documented aggregation policy in implementation tests. Downbeat evidence alone
does not certify meter or consecutive bar numbering.

A successful sample decode/load is independent of optional analyzer availability. Automatic
analysis settles its available/unavailable outcomes without waiting for model setup; PCM can
be published/playable. Manual analysis keeps current playback intact. Successful components
merge atomically with retained data and explicit statuses, rather than discarding a valid key
because an optional beat worker failed. Older serialized unified results restore as legacy
records; do not relabel them as Beat This or invent missing provenance.

Preserve legacy scalar mode, beat arrays, BPM overrides, signed grid offsets and loop markers.
Restore accepted cached Beat This results without its worker/model installed. Cache hits require
matching source and available provenance; legacy unknown provenance is retained as legacy,
not declared a verified new cache hit. Reanalysis creates raw new results without overwriting
accepted manual maps. Map adoption/edit migration belongs to the versioned-map/editor changes.

## Model acquisition and packaging

Setup is an explicit operation, separate from startup, loading, restore and inference. Pin the
package/environment lock, checkpoint `final0`, postprocessor/configuration and front-end
versions. Explicit author-host acquisition on 2026-10-05 produced 81,058,141 bytes, independently
hashed as `8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331`.
The accepted manifest retains origin/license evidence. Setup stages a verified runtime/model
and atomically selects it without removing earlier installations. This is an observed artifact
digest, not an author-signed checksum. Neither length nor HTTP ETag establishes integrity.

Normal analysis requires a verified local file and rejects missing/corrupt/mismatched weights
before calling upstream loading code, whose path/shortname fallback downloads. Do not allow
`torch.hub` code acquisition on an analysis request. CPU operation is mandatory for acceptance;
CUDA is optional, with the chosen device/precision recorded. Initial reference is FP32.

Beat This additions are optional/lazy. The current application still declares mandatory
Torch/torchaudio/Demucs; this slice neither removes nor proves absence of those dependencies.
Native ONNX/Rust execution remains a later parity/package investigation. It does not authorize
the complete Rust application port.

## Detection limits and refinement

The 50-fps front end ordinarily produces a 20-ms time grid. Sample-domain storage or playback
of those positions cannot guarantee sample-accurate musical beats. Keep raw predictions and
logits; logits are not calibrated correctness probabilities. Mark uncertain count, meter,
intro/break coverage and half/double interpretation instead of silently generating trusted
consecutive anchors. Four stems of the same source reuse one accepted map.

First benchmark the unmodified minimal postprocessor. A later, independently measured local
refinement can inspect bounded high-resolution onset evidence around selected candidates,
retain original/refined coordinates and reject ambiguous matches. Do not snap every beat to
the nearest transient: swing, syncopation, soft attacks and silent beats are valid. Manual
anchors specify intended musical positions with recorded revisions; count repair and local
timing adjustment are distinct operations. Automatic refinement is not activated by this change.

## Finite acceptance and cutover

B1a supplies the diagnostic adapter/PCM contract, job identities and missing-checkpoint tests
without weights/default activation. Full project checks and strict change validation passed
for this slice. B1b performs explicit setup and real Windows CPU reference
inference. B2 performs frozen private-track quality/resource acceptance and default cutover.
Freeze corpus, annotation uncertainty, correction-burden criteria and local resource limits
before tuning; report 10/20/40/70-ms errors, missing/extra beats, downbeat mistakes and long-track
count continuity. Training-corpus overlap may remain unknown.

B2 is a separate required cutover after B1b and its acceptance gates pass: route automatic/manual
NEW beat analysis through Beat This, update runtime/setup docs and verify old-project restoration.
Default transition does not itself adopt variable maps into live Quantize/SYNC. If acceptance
fails, record failed metrics and bounded remediation before cutover; do not call the replacement
complete. Rollback restores the prior new-analysis routing while retaining all saved data and
provenance. Missing-model handling after successful cutover never silently performs that rollback.

The separate qm-dsp capability is explicitly scoped to legacy/diagnostic use, retaining its
algorithm contracts without mandating the old backend for new analysis. Repair its sample-hop
bug when obtaining a trustworthy comparison; that does not block B1a. See the shared slice
ordering in [selected design](../../../docs/beatmap-sync-design.md).

## B2b1: Lossless bounded final publication

The B2a T05/R01 worker-only cases retained complete predictions but failed the
1-MiB final envelope cap because binary floating-point evidence was printed as
decimal JSON arrays. Final ready envelope version 2 uses four inline canonical
padded standard Base64 strings of uncompressed little-endian binary64 values,
with encoding `float64-le/base64`. This preserves arbitrary validated doubles
exactly; restricting the representation to float32 would quantize injected or
future valid binary64 values. Worker version 1, the 8-MiB response bound, the
250000-value array cap and the 1-MiB final bound remain unchanged.

The existing supervisor packs already validated terminal components outside the
callback. Rust bounds and decodes one array at a time, validates finite values,
source times and counts, then retains the compact envelope for the existing
atomic event path. It never expands numeric JSON at enqueue. The common Python
diagnostic reader accepts both final versions and reconstructs the full typed
predictions without PCM or model files. No external artifact, compression,
extra lifetime owner or alternate inference pipeline is introduced. Cancellation,
source identity and actual worker/key retirement retain their existing authority.

Packing is representation remediation, not permission to publish every possible
worker response. Oversize packed envelopes still report explicit beat failure
and preserve independent key output. Native long-track staging is addressed by B2b2 below;
independent labels, default adoption, saved-analysis restore and live SYNC remain
pending. Preserve the failed v1 evidence and report post-remediation worker-only
measurements with exact source/PCM/model lineage separately.

## B2b2: Complete staged PCM with separate source and key ownership

The frozen native 96-kHz T04/T05/R01 cases exceed the 512-MiB admission limit
when retained source, full mono, another loaded-rate key copy and key output are
counted together. Removing only the key copy is insufficient for the longest
source. Keep the cap and full-track algorithms; separate the ownership stages.

During export, native analysis pins all interleaved source channels and streams
the existing f64 arithmetic mean, rounded once to f32, through bounded buffers
to the complete loaded-rate little-endian mono file. No whole-track mono Vec,
second decode, silence trim or sample conversion through 44100/22050 Hz occurs.
Each exported frame must be bit-identical to the prior mono rule. Only a fully
written and flushed file with a readable retained native handle becomes prepared.
The analysis source pin then drops off-thread before key allocation; independent
loader/playback owners keep their original source and remain visible in RSS.

The native key reader consumes that same staged file in bounded chunks. At
44100 Hz it reads the exact mono values into one full vector. Other rates reuse
the existing `Fft<f32>` configuration (`1024`, one subchunk, one mono channel,
`FixedSync::Input`), continuous converter state and prior partial-input/tail
handling. Preserve the full output, remove leading delay exactly once and return
`ceil(source_frames * 44100 / loaded_rate)` frames. Compare every output sample
against the former full-buffer converter, including first/last impulses,
silence, chunk boundaries and fractional output lengths. CQT, ONNX inference,
KeyNet parameters, model identity and key outcome semantics stay unchanged.
At some fixed-input/FFT remainders a valid padding call initially emits zero frames.
Flush calls therefore use a finite dimension-derived allowance, with cancellation
between calls. The original full-buffer oracle rejects some such tails; those
newly supported cases compare against its explicitly zero-extended signal, cut
back to the original exact ceiling length. Keep the recorded original failure.
The worker independently reads the full loaded-rate file and retains its pinned
soxr HQ/log-mel/model pipeline.

Admission uses `max(export_stage_bytes, key_stage_bytes)` rather than summing
allocations that no longer coexist. Export includes the complete retained source
and every bounded export PCM buffer. Key preparation includes its complete output
allocation and all bounded read/conversion PCM buffers. Delay removal and tail
flushing operate through bounded converter output; the final vector has exactly
the ceiling-derived length and needs no full delayed-output allocation.
No live analysis pin or PCM allocation may disappear from the stage accounting.
Byte/frame arithmetic must check overflow and reject oversize work before the
corresponding allocation; the complete file is separately limited to 512 MiB.
Existing FFT scratch, CQT/ORT workspace and worker/model-memory exclusions stay
explicit. This reservation remains a PCM ownership bound, not a total-RSS claim.

Preparation failure/cancellation cannot expose a partial file to either reader.
Once prepared, key and beat work may run concurrently. Retain native read handles
through their actual key-reader lifetime and close them off-thread before final
file cleanup. The service retains the export and job directory until both key
and owned beat-process readers settle. Failed cleanup remains retiring. Native
KeyNet is still non-preemptible; cancellation/source replacement invalidates
publication immediately but cannot release a live key call or its slot. The
one-job/zero-queue policy, final identity lock and one-completion rule remain.

Non-goals: no default-analysis switch, saved-analysis adoption, live timing change,
new model/setup, alternate preprocessing, lossy publication, increased limits or
callback work. Full native T04/T05/R01 reruns must measure actual application plus
live worker RSS, complete output/publication, natural retirement and re-admission.
Preserve prior failures and frozen corpus/model/limits. Worker-only successes do
not close native resource acceptance; labels and default cutover remain separate.

## Primary evidence

- [Upstream release, weight license and training-data caveat](https://github.com/CPJKU/beat_this/tree/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c)
- [Version/dependency manifest](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/pyproject.toml)
- [Local loading, fallback download and chunk inference](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/beat_this/inference.py)
- [22.05-kHz front end](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/beat_this/preprocessing.py)
- [Minimal postprocessor](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/beat_this/model/postprocessor.py)

The [B1b report](../../../docs/beat-this-reference-evidence.md) records installed-source
reconciliation, executed Windows/reference checks and remaining quality/resource limits.
Source inspection and model parity do not certify musical accuracy or live synchronization.
