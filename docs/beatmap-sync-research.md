# Variable beatmaps, Quantize and SYNC: feasibility decision

Research date: 2026-10-05. Inspected implementation: `062fe8518e4cbb2021180ad5eac1c7af2abc9102`.
This is a sourced proposal, not implemented behavior or private-track acceptance. No neural
weights were downloaded, no new inference package was installed, and no audio was uploaded.
Current live timing, analysis and rendering remain unchanged.
The subsequent user decision selects Beat This as the replacement and adds future independent
per-pad KEY readiness. The [implementation design](beatmap-sync-design.md) is now the authoritative
staged plan; this report supplies its evidence and remaining technical limits.

## Recommendation

The concept is feasible as a staged architecture: one versioned source-beat map, one Rust master
clock, explicit initial-entry policy and continuous mapped playback. A model and a periodic
position check alone cannot deliver it. Beat This! 1.1.0 `final0`/minimal is now the selected
replacement for new analysis, with private-track and integration gates before default activation.
Correct the reproduced legacy downbeat defect when using it as a comparator; it no longer blocks
the new worker boundary. `small0` is an optional later footprint variant, not a silent substitute.

Use Rust for the shared forward/inverse coordinate evaluator; Python keeps project intent,
correction UX and background orchestration. Benchmark an isolated offline Rubber Band keyframe
renderer against the current source-reader/LiveShifter path before selecting live rendering.
A 500-ms observer should diagnose residuals, not periodically seek or reset audio.

The following drafts are real OpenSpec changes with uncompleted implementation tasks:

- [Offline map foundation](../openspec/changes/prepare-versioned-source-beatmaps/proposal.md)
- [Shared editor coordinates](../openspec/changes/share-editor-beatmap-coordinates/proposal.md)
- [Isolated rendering comparison](../openspec/changes/evaluate-variable-tempo-rendering/proposal.md)
- [Selected Beat This transition](../openspec/changes/adopt-beat-this-analysis/proposal.md)
- [Independent pitch/timing foundation](../openspec/changes/prepare-independent-pitch-timing/proposal.md)

These describe the selected future backend and staged contracts; none is implemented yet.
Existing clock/grid and Key Lock behavior remains live until its implementing slice passes.

## 1. Baseline: preserve what already exists, repair the comparator

The complete beat arrays already reach Python and project JSON. The loss of variable timing
occurs when display, snapping and runtime derive scalar BPM plus one origin, not in persistence.
Detected beats retain nonuniform DP positions; there is no subsequent constant-tempo fit.

| Boundary | Inspected implementation and consequence |
| --- | --- |
| Analysis | `audio_engine/mod.rs:349-409`: ODF, local tempo tracking, beat positions, average BPM and downbeats. Positions use ODF hops converted to f32 seconds. |
| Persistence | `models.py:115,296`, `controller/loader.py:742`, `controller/persistence.py:70,127`: full BeatGrid survives. No content fingerprint, detector version, confidence or correction lineage exists in SampleAnalysis. |
| Editor | `controller/transport/loop.py:91,176,261`, `ui/render/waveform_editor.py:466`: first anchor plus signed offset and `60/BPM` spacing. Auto-loop lengths deliberately ignore interior detected beats. |
| Runtime | `source_grid.rs:15`, `mixer.rs:1026,1359`: scalar source mapping and master/pad BPM ratio. SourcePlayback has constant-rate epochs with smoothing, not a variable map. |
| Launch | `audio_stream.rs:196`, `mixer.rs:527`: future grid/effective-loop-start remains live. Nearest-target and mapped starts are foundations/diagnostics. |
| Stems | `source_reader.rs:385`, `messages.rs:24`, `stem_cache.rs:130`: one source trajectory, validated immutable buffers and shared alignment correction already exist. Reuse them. |

Rust paths in this table are under `rust/crates/looper/src/`; Python paths under
`src/flitzis_looper/`. See [current architecture](architecture.md), which remains the runtime
reference. Same-source maps may be shared across pads, but independent pad triggers/voices are
not automatically one linked playback group.

### Reproduced downbeat defect

At `audio_engine/mod.rs:379`, `DownBeat::new(..., config.step_secs as usize)` receives zero
because the default is 0.01161 seconds. The ODF actually advances 512 samples at 44.1 kHz
(`analysis/src/detection_function.rs:39`). DownBeat expects that sample increment; multiplying
its beat indices by zero makes every spectral segment empty. All four phase scores tie.

A local Rust probe compiled the unmodified production DownBeat and math modules with the
already-built RustFFT library. Given 24 known beats, four synthetic accent patterns and silence
all returned indices `[3,7,11,15,19,23]`. Passing 512 restored audio-dependent answers, but some
accent phases were still wrong. This proves the units defect, not adequate detector quality.
The silence observation concerns DownBeat with supplied beat positions, not the full pipeline.

The integration fixture repeats the bad call (`analysis/tests/bpm_pipeline.rs:207`) and checks
nonempty/count rather than correct phase. Correct the sample-hop contract and seconds conversion
once, with meaningful position tests. Preserve existing persisted/manual grids; do not silently
reanalyze old projects. Record new analysis provenance when the versioned schema is introduced.
The native detector defaults to four beats/bar and clones downbeats into bars; it has no meter
sequence. Current analysis-hop precision is about 11.61 ms, not sample-level musical truth.

## 2. Detector comparison

Published scores are not directly comparable across training data, annotation conventions and
test splits. The selection below prioritizes inspectable, runnable, whole-track candidates.

| Candidate | Variable tempo / uncertainty | Code and weights | Runtime, availability and decision |
| --- | --- | --- | --- |
| Existing qm-dsp-derived Rust | Full nonuniform beat list; four-beat phase model, no calibrated uncertainty. Repair before comparison. | No neural weights. Upstream QM-DSP states GPL-2-or-later; preserve existing provenance/notices. | Already available on this Windows setup. Retain baseline/fallback. Accuracy and full-track cost must be measured. |
| Beat This! 1.1.0 `final0`, minimal postprocessing | Beat/downbeat sequence and logits without mandatory DBN meter/tempo priors. Not guaranteed consecutive true beats or meter labels. | Authors explicitly release code and published weights under MIT. | Selected replacement after acceptance; full mix, CPU or CUDA. Author host advertised 81,058,141 bytes; `small0` 8,451,101 bytes. Windows project compatibility untested. |
| Beat Transformer, published fold checkpoints | Five-instrument demixed input; reference DBN uses meter [3,4], 55-215 BPM. | Repository MIT; no distinct explicit weight-scope statement located. Confirm before selecting. | Eight published fold files, 37,235,863 bytes each, plus Spleeter/TensorFlow/Torch/madmom. Heavy comparator; four-stem caches are not the expected five-stem input. |
| madmom RNNDownBeat + DBN | 100-fps activations, configurable meter/tempo priors; transition penalty favors stable tempo. | BSD-2-Clause code; pretrained models/data separately CC BY-NC-SA 4.0. | CPU-oriented NumPy/Cython; eight model files about 3.125 MiB total. Modern Windows/Python packaging unverified; poor new default fit. |
| Masked Diffusion Beat This (2026) | Explicitly studies coherent beat counting and metrical consistency. | No license or deployable checkpoint found in inspected support tree. | Predictions/evaluation artifacts released; no runnable inference there. Relevant watchlist, not selectable backend. |
| BeatFM (2025) | Foundation-model adaptation for beat/downbeat tracking. | Task-checkpoint license/release not verified. | Paper available; no verified runnable task checkpoint found. Backbone availability does not establish backend availability. |

Primary evidence: [QM-DSP](https://github.com/c4dm/qm-dsp),
[Beat This release and weight statement](https://github.com/CPJKU/beat_this/tree/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c),
[Beat Transformer](https://github.com/zhaojw1998/Beat-Transformer/tree/063667fc9e4e11507f9d76dc1154d9db953a85eb),
[madmom license](https://github.com/CPJKU/madmom/blob/27f032e8947204902c675e5e341a3faf5dc86dae/LICENSE),
[madmom models](https://github.com/CPJKU/madmom_models/tree/7e3dc1b0cad499792767074d03c38b194b9b0a79),
[diffusion paper](https://arxiv.org/html/2608.04624v1),
[diffusion release tree](https://github.com/fosfrancesco/md_beat_this/tree/aca41a23a26881ba3d1e5b5e4bc3f4869d6a3ed7),
[BeatFM paper](https://arxiv.org/html/2508.09790v1).
These are upstream license statements, not a conclusion about distributing this application.
Beat This separately discloses training-data provenance/rights; its MIT statement for released
artifacts is not a license for the underlying music datasets. The five-stem and DBN details above
are visible in the reference [preprocessor](https://github.com/zhaojw1998/Beat-Transformer/blob/063667fc9e4e11507f9d76dc1154d9db953a85eb/preprocessing/demixing.py)
and [test configuration](https://github.com/zhaojw1998/Beat-Transformer/blob/063667fc9e4e11507f9d76dc1154d9db953a85eb/code/eight_fold_test.py).

Beat This uses 22.05-kHz mono, 128 mel bands and 441-sample hops: 50 fps, normally a 20-ms
timestamp grid. Minimal postprocessing selects peaks and associates downbeats with beats;
logits are not calibrated correctness probabilities. Its overlapping 1,500-frame chunks limit
model context, but a full spectrogram/logit array still grows with track length. This supports
whole-file offline analysis, not perfect bar continuity through every long break.
[Front end](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/beat_this/preprocessing.py),
[inference](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/beat_this/inference.py),
[postprocessor](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/beat_this/model/postprocessor.py).

The installed cost exceeds checkpoint size: Torch/torchaudio, NumPy, soxr, einops and rotary
embedding dependencies are declared. Keep inference optional and lazy, with explicit acquisition,
local-file loading, SHA-256 and license provenance. Reject a missing local checkpoint before
calling upstream code because its fallback downloads weights. Reuse decoded PCM to preserve
time origin; account for model resampling once. Benchmark exact Windows Python/runtime versions
(the app currently requires Python >=3.14); do not infer compatibility from a broad declaration.
[Manifest](https://github.com/CPJKU/beat_this/blob/b95c8ab0c58c2d9fcfd40508ae8dffbc05ac4f5c/pyproject.toml).

Author-host HEAD responses were 200 for
[`final0`](https://cloud.cp.jku.at/public.php/dav/files/7ik4RrBKTS273gp/final0.ckpt) and
[`small0`](https://cloud.cp.jku.at/public.php/dav/files/7ik4RrBKTS273gp/small0.ckpt).
Advertised lengths are availability evidence only; no payload/hash/inference was verified.
No RAM/VRAM, analysis time or installed-footprint number is claimed for this computer.

### Native portability is plausible, not yet verified

Actual third-party exports exist. The inspected C++ port documents Windows/ONNX Runtime and
an opset-14 converter; the Rust port has rten/ort and an opset-17 exporter. Neither was executed
here. The Rust parity test uses one MP3 and F1 with a 70-ms matching window, logs rather than
asserts exact timing, and can skip the full model when missing. Its CI is Ubuntu-only. That is
stronger evidence than speculation about export, but weaker than Windows or exact-time parity.
[C++ converter](https://github.com/mosynthkey/beat_this_cpp/blob/07ab790a9ec2eda8093d52d249e3ec4f0510ee72/onnx/convert_to_onnx.py),
[Rust exporter](https://github.com/danigb/beat-this-rs/blob/1ae768e78f1ad83b0ed3886241dc29ffde853c40/scripts/ckpt2onnx.py),
[parity test](https://github.com/danigb/beat-this-rs/blob/1ae768e78f1ad83b0ed3886241dc29ffde853c40/tests/python_parity.rs),
[CI](https://github.com/danigb/beat-this-rs/blob/1ae768e78f1ad83b0ed3886241dc29ffde853c40/.github/workflows/ci.yml).
The Windows support statement is in the [C++ port documentation](https://github.com/mosynthkey/beat_this_cpp/blob/07ab790a9ec2eda8093d52d249e3ec4f0510ee72/README.md).

Future export gate: pin one checkpoint and FP32 oracle; compare PCM/resampling, mel features,
logits, peak counts and times including chunk edges, short files and long files. Measure Windows
CPU startup/RAM/cancellation offline and on a clean machine. Native inference for this detector
does not solve separator portability or authorize the full Rust application port.

## 3. Shared coordinates and storage

Let `M(n)` be master beats at absolute output frame n. A trusted map contains increasing anchors
`(s_k,b_k)` in source frames and explicitly defined beat units. On a segment:

```text
B(s) = b_k + (s-s_k) * (b_(k+1)-b_k) / (s_(k+1)-s_k)
S(b) = s_k + (b-b_k) * (s_(k+1)-s_k) / (b_(k+1)-b_k)
source(n) = S(b_loop + mod(M(n)+phi-b_loop, L_beats))
r(n) = d(source)/dn = (source_rate/output_rate) * master_BPM/local_source_BPM
```

Here `b_loop=B(loop_start)`, `L_beats=B(loop_end)-b_loop>0`, and mod is Euclidean modulo,
including for negative signed phase. The derivative excludes the explicit loop-wrap discontinuity.
Loaded buffers currently have
the output rate, making the rate factor one. A beat must have a defined musical unit; do not
assume every detector pulse in 6/8 is a quarter note. Preserve meter evidence, but initially
offer sustained bar-sync only for reviewed compatible 4/4 maps/loops. Other audio stays playable.
Missing/extra detections must be corrected or marked uncertain before assigning trusted indices.

At Quantize target T, choosing `phi = B(loop_start)-M(T)` associates that source position with
T. Add an explicit creative offset `phi_creative` in beats if intended. Positive phi advances
source phase in this convention. For an already aligned map, preserve its chosen phase instead
of reinterpreting every loop start as a downbeat. This distinction is a launch intent, not
something a nearest 1/16 boundary can infer.

Editor global alignment changes source-to-grid mapping; it does not change M. Keep that
translation separate from local anchor correction and creative playback phase. A musical-axis
view may visually move the waveform under fixed beat lines; a source-time view shows nonuniform
grid spacing. Both query the same evaluator. Swing and syncopated events between anchors remain
musical content, not extra beat anchors to flatten automatically.

Persist f64 source seconds relative to canonical decoded PCM plus beat indices, unit, meter,
confidence/coverage, schema/revision, content fingerprint, preprocessing, detector/model/config
and manual edit lineage. Derive loaded-frame maps once. Path/size/mtime is only a fast identity
hint. Legacy f32 times retain their original precision after conversion. Store raw detections
apart from accepted maps; keep a separate exact legacy scalar mode and existing manual offsets.
Outside coverage, extrapolation is visibly unverified; no hidden trusted grid across silence.

New maps/corrections are immutable revisions. Validate count, spacing, finite monotonic anchors,
source extent, inverse and metadata off callback. Native control-side queries serve the editor.
Later live adoption needs prepared bounded segment lookup, a hard worst-case lookup budget,
source/map/loop/tempo identities and off-thread retirement via existing ownership mechanisms.
No callback full-track scan, vector construction, final destructor or inference is acceptable.
Initially defer edits affecting playing audio to stop/retrigger and show pending revisions.

Raw stem caches remain valid when only a map changes. Warped derivatives need distinct identities
including map/manual revision, loop, rate, target tempo trajectory, renderer build/options and
stem provenance. Publish the complete accepted group atomically; reject partial/stale results.
Cache cancellation, source replacement and old-project restore reuse current loader safeguards.

## 4. Rendering choices and stem limits

Current audio sums selected source stems before a single stereo LiveShifter. It uses varispeed
read ratio `r` and pitch compensation `1/r`. Preprocess both comparison paths to the same sample
rate, as current loaded buffers already do. Full RubberBandStretcher then consumes original
source with duration ratio `q=1/r` and pitch scale 1 for KEYLOCK. For unequal frame rates the
duration-ratio formula is `q=(source_rate/output_rate)/r`. Applying both tempo changes to
the same feed would process tempo twice. Native delay, feed cursor and audible cursor differ.

| Route | Advantage | Open risk / recommendation |
| --- | --- | --- |
| Map reader + current LiveShifter | Reuses source/mask selection and existing controls. | Existing 0.05/512-frame smoother does not preserve anchor integrals. Variable-pitch history, neutral crossings and audible phase unproven. |
| Full realtime Stretcher | Direct time/pitch separation with dynamic ratios. | New bounded variable-output adapter, allocation audit and feed/output timeline needed. Defer until isolated evidence. |
| Offline keyframe warp to fixed target | Preparation and DSP startup finish before callback playback. | Rebuild on tempo/map edits, storage, seams and independent-stem artifacts. First diagnostic candidate, not chosen runtime. |
| Offline reference-tempo normalization + live stage | Can reuse normalization across master tempos. | Two processing stages can compound artifacts; live latency/launch limits persist. Contingency only. |

RubberBandStretcher keyframes map source frames to output frames and are offline-only; total
duration ratio is separate, reset clears the map. Its realtime mode has qualified allocation
guarantees and process-size/rate-change constraints. Native API options differ from LiveShifter.
[v4.0.0 Stretcher API](https://github.com/breakfastquay/rubberband/blob/v4.0.0/rubberband/RubberBandStretcher.h),
[C API](https://github.com/breakfastquay/rubberband/blob/v4.0.0/rubberband/rubberband-c.h).

The integration guide requires handling variable output availability and realtime padding/delay;
changing an input ratio is not an immediate audible-output change. R3 internally updates maps at
processing hops, so supplied anchors are not proof of sample-exact attacks. Source inspection
also suggests a first explicit 0-to-0 keyframe hazard; this was not reproduced and is not a
claimed universal bug. Start the isolated test with implicit origin/positive interior anchors,
then test invalid/endpoints deliberately.
[Integration guide](https://breakfastquay.com/rubberband/integration.html),
[pinned map implementation](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3Stretcher.cpp#L413).

Derive position from the map and construct a bounded positive rate trajectory whose integrals
reach the anchors. Any accepted smoothing must define a versioned canonical B/S interpolation
shared by editor, preparation and renderer; an alternate curve is only a labeled diagnostic.
Endpoint agreement alone does not ensure agreement between beats. Simply smoothing successive
rates causes phase debt. Map slope discontinuities,
large rate jumps and out-of-range ratios need an explicit reject/degraded policy, not silent
clipping with a SYNC indicator. Preserve native quality/options/resampler in comparable experiments.

Four stems of one source share map, source reads, wraps, trajectory and revision. This guarantees
coordinate agreement, not that independent nonlinear warps sum to the warped full mix.
CHANNELS_TOGETHER is not an eight-channel coherence guarantee; R3 mid/side paths require exactly
two channels. Compare stem-sum then one processor, separate stereo processors then sum, full-mix
warp and an experimental multichannel path. Measure separator residual separately from added
warp residual, audible transient displacement and mono compatibility.
[R3 channel handling](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3Stretcher.h),
[LiveShifter API](https://github.com/breakfastquay/rubberband/blob/v4.0.0/rubberband/RubberBandLiveShifter.h).

Loop in musical space with explicit half-open endpoints. Test contiguous multi-cycle renders
and central-cycle extraction; do not assume native state becomes periodic. A seam fade is an
explicit content-altering policy, not a way to hide failed onset/retention criteria.

## 5. Proposed independent controls

This matrix is a recommendation requiring a later behavior delta and acceptance. All four
combinations are useful; KEYLOCK controls pitch preservation independently. BPMLOCK retains its
scalar fallback role when SYNC is off. When SYNC is on it must have sole rate authority, rather
than multiplying its map ratio by BPMLOCK a second time. Preserve the user's BPMLOCK intent for
returning to SYNC-off. MULTILOOP governs simultaneous voices, not master clock ownership.

| Quantize | SYNC | Initial entry | Following playback |
| --- | --- | --- | --- |
| Off | Off | Earliest reactive effective-loop-start, subject to actual DSP availability. | Existing free/global-speed or scalar BPMLOCK behavior. |
| On | Off | Captured target plus phase-appropriate source catch-up at earliest legal entry. | Continue under chosen free/scalar rate; variable-tempo drift is expected, no ongoing correction. |
| Off | On | Earliest legal entry at current mapped master phase; can begin inside the loop. | Continuous map trajectory with intentional phi preserved. |
| On | On | Targeted musical entry and the same causal catch-up rules. | Continuous mapped tempo/phase, including master tempo changes. |

Quantize-only aligns entry position; it cannot keep changing-tempo material aligned after the
drop. Quantize-off/SYNC-on cannot simultaneously promise immediate loop-start, no skipped content
and current master phase unless they happen to coincide. Recommend current phase for that mode;
make start-from-beginning a deliberate alternate intent rather than a periodic corrective seek.

### Subdivision, tolerance and bar intent are different

For a note denominator d, grid spacing is `delta_ms=240000/(BPM*d)` in the current quarter-note
master. At 120 BPM, 1/16=125 ms, 1/32=62.5 ms and 1/64=31.25 ms. Symmetric nearest selection
allows +/-half that interval and sends exact midpoint ties to the future. Captured input time,
not delayed UI handling time, selects the intended target; exact hits keep the same boundary.

The requested example (early half a 1/16, late almost a full 1/16) is asymmetric. Illustrating
"almost" as 120 ms gives a window [-62.5,+120] ms at 120 BPM. Adjacent 1/16 target windows
overlap by 57.5 ms. A press can therefore belong to two targets; tolerance alone cannot identify
the intended one. This is a mathematical ambiguity, not clock inaccuracy.

Recommendation: preserve nearest/tie-future for subdivision launches; allow that asymmetric
window only around an explicitly selected beat/bar-downbeat target. Then define overlap/tie and
outside-window policy deliberately. Do not silently treat a 1/16 trigger as a request for "1".
Retain existing grid options initially; 1/8 and 1/4, a 1/16-only default or removal of 1/64 remain
product choices, not necessary accuracy fixes.

### Late entry and sufficient preparation

For proposed phase-first Quantize/SYNC entry, T is the target frame selected from captured input
(or current mapped phase for immediate SYNC). E is the earliest frame still
renderable with a ready source-specific state and actual queued/device horizon accounted once.
An early request can wait for T if ready. A late request enters at E and evaluates the source
phase advanced by `M(E)-M(T)`; elapsed content is skipped. Quantize-only switches to its chosen
free/scalar progression at actual entry E. Off/off retains reactive loop-start and is exempt
from mapped catch-up. Already buffered or physically emitted audio cannot be changed.
Stale/missing clock evidence uses an explicit degraded fallback with
no claim of audible alignment. Never replay elapsed sound to pretend the start happened at T.

The old fixture has C3678/U2080 at 48 kHz, requiring 1598 frames = 33.292 ms before T to retain
the tested native pre-target response. Strict first sound at T discards that content even with
unlimited preparation. New maps do not remove this causal limit; a different renderer must be
measured, not assigned the same delay or assumed exempt. All original failed gates remain failed.
See [the retained causal evidence](key-lock-backend.md).

An alternate protected-attack intent chooses the first target with enough headroom:
`T_future = first compatible grid boundary >= E+H`. Here H must come from readiness and measured
content policy, not a universal delay constant. If preserving pre-target response requires early
emission, it must be explicitly allowed for this alternate intent; a future target alone does
not make strict gating preserve the old response.

Example only: original T=0, E=40 ms, H=33.292 ms, 120 BPM, allowing the required pre-target output:

| Grid | New target | Additional target wait after E |
| --- | ---: | ---: |
| 1/4 | 500 ms | 460 ms |
| 1/8 | 250 ms | 210 ms |
| 1/16 | 125 ms | 85 ms |
| 1/32 | 125 ms | 85 ms |
| 1/64 | 93.75 ms | 53.75 ms |

Target wait is not first-sound wait. Only a matching bar "1" can also require up to a full bar
(2 seconds at 120 BPM). Recommendation: phase-first catch-up for the intended performance drop,
with the lost-attack consequence explicit; evaluate protected future entry as a separate option.
Do not fit another bridge or claim both policies meet the old incompatible retention gates.

### State transitions and monitoring

| Event | Recommended future contract |
| --- | --- |
| Stop/unload/queue-full | Cancel matching pending work; reject new work without stopping unrelated voices. No stale adoption. |
| Pause/resume | SYNC-off resumes saved cursor; SYNC-on resumes at current mapped phase. This difference needs visible intent and tests. |
| Seek/retrigger | Explicit seek updates phase intent or exits SYNC; preserve that intent at the next observer tick. Retrigger uses the one launch policy. |
| Map/source/loop/tempo revision while pending | Existing scheduler targets stay fixed. A new synchronized prepared event must validate captured identities; if obsolete, cancel/report instead of adopting wrong state or silently rerounding. |
| Accepted master BPM | Preserve complete M at transition, then follow new tempo; reprepare future trajectories. Never make another pad the master implicitly. |
| Unsupported map/rate | Reject activation or report loss of lock and retain a documented safe mode. Never display locked after silent clipping. |
| Bootstrap | Reuse selected-reference one-time bootstrap; no new "first playing pad" clock election. |

Every 500 ms, a supervisor may inspect timestamped Rust snapshots: map/voice identity, expected
phase, actual source/feed provenance and estimated output position. Compare the same time domain
and subtract creative phi. One percent rate error accumulates 5 ms in 500 ms and 18 seconds in
30 minutes. Monitoring can expose it; it is not a source clock. Normal synchronized progression
should have no accumulating phase debt. Any small correction needs bounded slope, settling-time
and audible tests; large errors require explicit future adoption/relaunch, not hard reset.

## 6. Local benchmark and measurable gates

No private-track neural benchmark or rendered-warp benchmark has run in this research step.
Use local decoded audio and exports; do not upload samples to a demonstration service.

Freeze a manifest before candidate tuning: source hashes, decoded rate/origin, exact segment
ranges, reference annotation version, model/config hash, hardware/driver and all test parameters.
Use at least two distinct recordings per class where available: constant electronic, vinyl drift,
ramps, abrupt tempo changes, half/double ambiguity, sparse intros, breaks, swing/syncopation and
difficult downbeats/meter. Separate tuning and held-out tracks. Keep long complete tracks too;
cropped excerpts hide beat-count switches. If a class lacks private audio, label that gap and use
synthetic fixtures for arithmetic only.
Held-out local tracks prevent tuning on the acceptance set; they do not prove that a recording
was absent from a model's training corpus.

Annotate before comparing model output. Use manual beat/bar anchors, uncertainty intervals and
metronome listening; adjudicate ambiguous regions instead of inventing ground truth. Record
correction operations/time, not only a score. Beat evaluation literature includes correction
effort precisely because raw detection scores need not reflect editing cost.
[Correction-effort study](https://arxiv.org/abs/2011.01637).

Report separate beat/downbeat precision/recall/F1 at 10/20/40/70 ms, signed bias, p50/p95/max
matched error, missed/extra beats, metrical switches, longest correct span, bar numbering and
manual repairs. Standard F1 uses 70 ms and conventional evaluations may trim the first five
seconds; include untrimmed startup/break results here. Continuity scores supplement F1 but
alternative-metrical-level scores must not hide wrong quarter-note/bar interpretation.
[mir_eval documentation](https://mir-eval.readthedocs.io/latest/api/beat.html).

| Gate | Proposed measurable criterion / limitation |
| --- | --- |
| Baseline repair | Reproducer fails before, unit-correct path is audio-dependent after; known phase assertions and sample-clock conversions tested. No automatic cache overwrite. |
| Map arithmetic | Strict validation; forward/inverse agreement <=1 loaded source frame at anchors; same results across partitions/rates and long musical loop counts. This is a numerical gate. |
| Detector eligibility | No unflagged count/meter discontinuity in accepted regions; candidate reduces held-out manual correction burden without regression on critical downbeats. Freeze numeric dataset targets before tuning. |
| Model resources | Measure cold/warm full-track time and real-time factor, peak RAM/VRAM, installed/model/cache size, CPU fallback, cancellation and concurrent playback impact. Local limits not yet measured. |
| Render quality | Measure exact-map markers, original onset/peak/energy gates, seam clicks, stereo/mono and stem-mask behavior; add listening to continuous music references. Coordinate success is insufficient. |
| Realtime adoption | Zero forbidden callback operations, bounded feed/retrieve/map work and off-thread destruction; 1/4/8 voices, ratios/modes and cold transitions. At 512/48k callback budget is 10.667 ms; record p95/p99/max/misses. |
| Device/long run | Loopback/recorded onset plus driver conditions, 30-minute reference session, no accumulating residual, missed beats or hidden correction resets. Original acoustic failures cannot be relabeled passes. |

The only new executed probes are (a) the isolated DownBeat reproduction described above and
(b) arithmetic checks of inverse/loop coordinates, asymmetric-window overlap and future-wait
examples. These use no neural model or audio device and do not satisfy the later quality gates.
Detailed local evidence and reproducible commands remain in workspace scratch, not committed audio.

## 7. Implementation sequence after the user's selection

Use [the selected implementation design](beatmap-sync-design.md#ordered-implementation-slices):
B1a offline Beat This boundary; B1b explicit model/oracle setup; B2 new-analysis default after
acceptance; B3 shared map; B4 sample-domain correction/editor; B5 independent time/pitch render
proof; B6 live mapped ownership; B7 Quantize/SYNC; B8 live release. Exact-map foundation and
render experiments can proceed independently of unfinished model/editor UI work as stated there.
The old R0-first/model-selection gate is superseded. The legacy defect remains recorded and
must be repaired before using it as a trustworthy comparator.

Future per-pad KEY UI is K1, using the already tested independent pitch contract. Its transposition
must not modify M, B/S, phi, source playheads or another pad's timing. Combined pitch/tempo ranges,
unity crossings, pitch-conditioned latency and revision-safe handover are required now, not a
later retrofit. Current source-key metadata correction remains separate.

Launch target/tolerance and future KEY range/KEYLOCK-off UX remain explicit later product choices;
they do not reopen the selected Beat This direction or block the offline foundation. Existing
optimization/acceptance, separator replacement and full Rust-port planning slices remain required.
Do not silently replace active launch behavior or cross the Rust-port implementation stop gate.

## 8. Evidence boundaries

This report establishes an existing defect, an inspectable model shortlist, concrete native API
paths and a reviewable architecture. It does not establish a winning model on the user's music,
Windows native-model parity, accurate downbeats after one-line repair, acceptable variable warp,
cross-stem coherence, measured hardware latency or long-session acceptance. Runtime docs remain
unchanged because implementation remains unchanged. Revisit this decision record after measured
results; move accepted architecture into maintained runtime docs in the implementing slice.
