# BPM analysis

Automatic BPM produces a source-bound constant scalar. Playback uses the existing
[frame clock and scheduler](runtime-and-transport.md); analysis never becomes a
second clock. [Key classification](key-analysis.md) has independent outcomes.

## Productive path and resources

[AppController's loader binding](../../src/flitzis_looper/controller/loader.py)
configures the verified Beat This installation. The productive native owner is
[app_analysis.rs](../../rust/crates/looper/src/audio_engine/app_analysis.rs), with
selection in [app_tempo.rs](../../rust/crates/looper/src/audio_engine/app_tempo.rs).
An unconfigured native scientific caller uses the existing `analyze_sample`
route; that is distinct from silently replacing failed configured neural work.

Windows x64 uses the pinned dynamic Beat This `final0` ONNX graph in FP32 with
the existing soxr HQ, log-mel, chunking and minimal postprocessing.
[neural_analysis/raw.rs](../../rust/crates/looper/src/audio_engine/neural_analysis/raw.rs)
binds request/model/input geometry. Python
[neural_worker.py](../../src/flitzis_looper/analysis/neural_worker.py) verifies an
existing installation and runs an isolated worker off the main/UI/audio thread.
[Worker setup](../model-worker-setup.md) provisions the graph and the CPU/CUDA
runtime. Analysis can prepare a stale CUDA cache using the installed package.

ONNX Runtime 1.24.4 runs on a usable NVIDIA GPU or on CPU with the same graph.
A qualified CUDA error can start one fresh CPU worker after CUDA and its readers
retire. The result records the actual device and fallback reason. Inference
failures can yield a rough BPM estimate with an empty grid and retained diagnostics.

## Complete-source preparation and admission

[Media and PCM](media-and-pcm.md#import-and-complete-disk-artifacts) owns immutable
originals, decoder/playback identities and finite playback residency. Analysis
preflights complete geometry before materializing its complete PCM input. It does
not analyze only the visible loop or silently crop a long source.

Mandatory BPM preparation uses checked geometry, current host memory and fallible
allocation for temporary buffers. The worker observes actual OS memory pressure.
Key admission remains independent. Export streams the complete engine-rate mono
source into a sealed f32le
temporary file, binding extent and SHA256. Beat This and local Key work may overlap;
the original sample and neural wire remain held and accounted. Dropping a cloned
snapshot does not release another caller's retained source.

Key converts the complete mono input to its own 44.1 kHz file and uses the bounded
[file-backed frontend](key-analysis.md#complete-file-frontend-and-memory). A Key
budget refusal cannot veto an otherwise admitted BPM request. Conversely, finite
playback residency does not grant unlimited complete analysis allocation. This
admission is not process-tree RSS, model-arena or OS-file-cache accounting.

The borrowed Native job stays alive until worker/descendant/pipe/temp readers are
actually retired, including cancellation. Late responses cannot replace newer
source/content/request ownership. Complete conversion validates finite data,
extent, resampler delay trim and tail; zero-output padding is not automatically
evidence that no progress is possible.

The supervisor claims the child before attaching output readers. Cancellation
or reader setup failure retains the shared heavy permission and scratch/source
owners through process-tree and reader retirement. A refused admission before
launch settles without creating a child or treating contention as model failure.

## Constant-tempo selection

The productive first policy is `neural-stable-regions-v4`, not the scientific
selected-backend summary. It fits the middle 20–80% and temporal thirds of the
complete source, preserving raw observations. Regional support requires at least
24 points and 30 seconds; independent nonnested corroboration uses a relative
BPM agreement of 0.00025. The short whole-source route needs eight points and
four seconds. Stable-run recovery can supply a supported candidate when the
fixed regions cannot. These are engineering support rules, not musical labels.

A nearby integer counterprobe shares one original-event phase and separate probe
windows with the fractional candidate. Each retains ten seconds when source
extent permits; short sources use the existing adaptive check. Unsupported integer
proximity never rounds away the fractional result. If policy4 has no supported
result, the current `neural-inferred-periods-v9` recovery assesses pulse/ordinal evidence.
Budget, allocation and search limits mean incomplete work, not conflicting tempo.

[app_analysis_qm.rs](../../rust/crates/looper/src/audio_engine/app_analysis_qm.rs)
allows independent QM corroboration only for the qualifying phase-only refusal.
[app_analysis_mean.rs](../../rust/crates/looper/src/audio_engine/app_analysis_mean.rs)
allows the final approximate mean only after precise work completes with
`conflicting_sections`. Three interior sections must independently support a
common pulse family in both detectors, with the same half/double normalization,
local coverage/residual/outlier checks and at least 30 seconds each. Elapsed-time
weighting yields the middle-60% mean. Insufficient or unfinished work yields no mean.
The learned nominal TempoCNN prior is test-only, excluded from fresh production.

After unsuccessful precise attempts, [app_analysis_approximate.rs](../../rust/crates/looper/src/audio_engine/app_analysis_approximate.rs)
and [app_tempo_approximate.py](../../src/flitzis_looper/analysis/app_tempo_approximate.py)
implement the separate version-11
`source-approximate-tempo-v11` policy, which returns a scalar-only fallback. It first
reuses a usable retained neural period (`60 / period_seconds`), then supported
median beat intervals. If neither is available, including unavailable inference,
it computes source-energy novelty in at most three fixed windows of up to 20
seconds, using approximately 100 bins per second and 30–300 BPM autocorrelation.
Smoothed, centered novelty requires repeated rhythm support; silence, constant
signals, corrupt or too-short input and cancellation cannot supply a default BPM.
All work remains on the admitted analysis worker, outside the audio callback.

The strict Rust/Python contract replays the coarse scalar separately from its
unchanged original precise refusal. It retains source/mono hashes, geometry,
model configuration, actual execution when available and the captured range.
Source-energy evidence retains bounded window coordinates and novelty vectors;
it does not invent a neural execution receipt. Selection precedes range filtering,
with no clamping, rounding or range-driven harmonic preference. A failed saved
v4/v9 estimate can be replayed after Native verifies the current original digest,
without repeating model inference. A valid same-source precise tuple wins.

The method JSON and scalar are selected together. A failed optional attempt keeps
the exact base method record. [app_analysis_range.rs](../../rust/crates/looper/src/audio_engine/app_analysis_range.rs)
validates an optional finite 30–300 BPM range with min less than max. An already
qualified in-range value stays; another value must independently qualify. It is
not an arithmetic fold into the range. QM period-double corroboration also bounds
accumulated drift to 5 ms over the common evidence extent.

## Precision, uncertainty and live timing

Analytical periods, scalar BPM and source coordinates retain binary64 precision.
Legacy/display beat arrays can contain binary32 values; they do not redefine the
selected analytical period. Physical loop markers are integer source frames,
while musical periods can be fractional. See [grid and source timing](grid-and-source-timing.md).

An approximate mean exposes **Possible BPM drift** and **Mean estimate · middle
60%**. It is one constant playback estimate, with no source-phase following.
Confidence/support, reproducibility and extra decimals do not prove correct
musical counts, acoustic phase, constant recording speed or device alignment.
Detector disagreement does not identify its physical cause.

The general fallback displays **Approximate BPM** on the PAD and **may be
inaccurate** in details. Save, restore and copy retain its evidence and warning.
It supplies no beat grid or accepted timing; diagnostic observations remain only
in provenance. A defensive timing-anchor guard prevents those observations from
becoming a source phase. Manual/Tap, half/double and explicit Grid Offset remain
performer choices. This fallback does not establish musical ground truth.

Python [transport/bpm.py](../../src/flitzis_looper/controller/transport/bpm.py)
and [loader.py](../../src/flitzis_looper/controller/loader.py) preserve manual/Tap
precedence and a supported precise result against a later mean on the unchanged
source. A failed/filtered reanalysis can retain prior valid metadata; new content
cannot inherit another source's result. An independent Key result may still merge.
Saved method/range describes the actual request, not today's settings.

Current timing publication rechecks source/content/generation and separate timing
epochs. Accepted constant evidence and its derived loop/master refresh have
operation-specific OwnACK paths in
[accepted_publication.py](../../src/flitzis_looper/controller/accepted_publication.py).
Saved analysis, raw arrays, enqueue, fitted intercepts and complete OLS summaries
cannot certify accepted origin, current source adoption or audible phase.
BPM Lock remains independent of MultiLoop with the established stable master.

## Manual, Tap and diagnostic boundaries

Manual BPM, /2, x2 and Tap use source-qualified controller admission before saved
intent changes. Tap fits the indexed complete current series iteratively, rejects
strong outliers and can reinstate them as later evidence changes. It is not the
mean of adjacent tap intervals. A pause longer than three seconds or source/PAD
change resets the series. Systematic Human timing drift cannot be inferred away.
Grid Offset is separate phase intent; BPM alone cannot choose it.

[OfflineAnalysisService](../../src/flitzis_looper/analysis/jobs.py) borrows the
Native diagnostic job to export complete source PCM and obtain a matching
terminal result. Its snapshot exposes progress, BPM/Key outcomes and optional
diagnostic summary. After component and PCM retirement and scratch cleanup,
Native finish checks the current source and request before publishing the
terminal result and releasing admission. The
[Native summary binding](../../rust/crates/looper/src/selected_bpm.rs) assesses
method-specific metadata.
[Selected-backend fitting](../../rust/crates/analysis/src/selected_bpm/mod.rs)
supports complete-sequence and representative-region summaries. The
[tempo summary kernel](../../rust/crates/analysis/src/tempo_summary/mod.rs)
binds count/period hypotheses to complete evidence and reports conditional
precision. QM raw coordinates use the detector's actual hop and resampler
timebase. An ordinal fit assumes the count mapping. Independent reference
inputs, source aliases and sealed scoring are handled by the
[offline scoring CLI](../../src/flitzis_looper/analysis/beat_scoring_cli.py).
None of these interfaces adopts timing by itself.

See the [analysis](../../openspec/specs/audio-analysis/spec.md) and
[manual BPM](../../openspec/specs/pad-manual-bpm/spec.md) contracts.
The [evidence index](../../openspec/changes/archive/2026-10-30-evidence-index.md)
retains independent-label, scoring and correction-session evidence and limits.
