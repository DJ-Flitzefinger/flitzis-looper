# Beat This B2a finite acceptance

Protocol: `b2a-private-pilot-v1`, frozen 2026-10-05 before new inference or tuning.
The selected configuration remains Beat This 1.1.0 `final0`/minimal, CPU FP32.
This document defines the gate; it does not announce musical acceptance or a
new-analysis default switch. [Reference evidence](beat-this-reference-evidence.md)
establishes B1b implementation parity, separately from this gate.

## Frozen private corpus

The local manifest is `scratch/b2a/frozen-manifest.json`, SHA-256
`df0d3e62731c58383c83fbb7f6474f6534648eb873d1045f6238b81d28fe8ac5`.
It records all six original file hashes, sizes, container durations/rates/channels,
the exact model/environment/native hashes, hardware evidence and numeric limits.
Private filenames, audio, annotations and predictions remain outside Git.

| IDs | Frozen role | Full original duration |
| --- | --- | --- |
| T01, T02 | Development references | 240.943 s, 170.631 s |
| T03, T04, T05 | Proposed held-out references | 212.376 s, 357.776 s, 518.534 s |
| R01 | Long-track resource reference only | 600.500 s |

These are complete recordings, with no conventional first-five-seconds exclusion.
Container durations are inventory data; evaluation uses the complete native loaded
frame extent and origin zero. A filename suggesting 120 BPM is not a beat label.
The split is provisional until a human confirms recording groups: excerpts,
derived files and repeated material in compilations cannot straddle the split.
Musical classes must also be independently identified. Missing drift, ramps,
abrupt changes, swing, sparse sections or meter cases remain explicit coverage
gaps; this finite pilot cannot establish general variable-tempo acceptance.

No model parameters, postprocessing thresholds or acceptance limits may be tuned
on held-out results. A necessary protocol revision records its reason and a new
manifest identity; it does not retroactively turn a failed v1 result into a pass.

## Independent labels and scoring

At freeze time no independent full-span beat/downbeat annotations were present.
Existing legacy arrays and B1b predictions are not ground truth. The user's
manually verified grids can supply source BPM and signed start/bar anchors, with
their actual verification scope recorded. A scalar grid or a few spot checks
cannot certify every beat count or a variable-tempo trajectory.

Seal annotation revision/checksum before the annotator inspects the new raw model
results. Retain original source hash and the exact loaded PCM origin/rate/hash,
or a separately verified decode-to-loaded mapping. MP3 gapless decoding can alter
the beginning/tail; never fit an offset to improve model scores. Do not cut away
startup ambiguity or replace original test files. Negative grid origins are valid;
the first visible grid line need not be a beat at source time zero.

Each claimed full span needs independent beat times, quarter-note/count
interpretation, bar/downbeat identities, meter, timing uncertainty and explicit
ambiguous/nonrhythmic intervals. Freeze those intervals from listening rather
than expanding exclusions after a failed prediction. Require at least 90% confident
annotation coverage, and report trusted metrical coverage separately; a confirmed
nonrhythmic region is annotated but does not define a beatmap.

The pre-inference scoring clarification is sealed separately in
`scratch/b2a/scoring-addendum.json`; its hash is retained in the adjacent SHA-256
file. It leaves the original corpus and numeric resource targets unchanged.
Use monotone one-to-one maximum-cardinality matching, then minimum summed absolute
error, then the lexicographically smallest sequence of reference/prediction index
pairs, separately for beats and downbeats. Report precision/recall/F1 at
10/20/40/70 ms, signed timing bias, absolute p50/p95/max, missing/extra counts,
longest correct span and startup/break/tail results. Report uncertainty widths
and the excluded-label count for each tolerance. Labels broader than a tolerance
cannot establish that timing precision; interval-distance scores are companion
evidence, not substitutes for precise labels. Maximum eligible label uncertainty
half-width is 2.5/5/10/10 ms for the four respective tolerances. At least 90% of
independently annotated metrical beats and downbeats must be eligible at both
40 and 70 ms. Report their denominators separately from temporal coverage;
otherwise broad uncertainty could hide difficult events while retaining a high score.

Every complete musical reference must meet:

- Beat F1 at 40 ms >= 0.90 and at 70 ms >= 0.95.
- Downbeat F1 at 40 ms >= 0.80 and at 70 ms >= 0.90.
- Missing beats/reference beats <= 2%, and extra beats/reference beats <= 2%,
  separately at 70 ms. These count gates are deliberately stricter than the F1 gate.
- No half/double quarter-note switch, wrong bar phase or run of two or more
  unmatched consecutive beats outside the independently declared gaps. Aggregate
  F1 or alternative-metrical-level scores cannot excuse these failures.
- Every independently selected critical downbeat retains its intended bar identity
  within 40 ms. Select the first unambiguous bar, post-break/re-entry and late-track
  bar on each held-out recording where those features exist; record absent features.

## Held-out manual correction burden

Record inserts/deletes/moves, count/meter/phase corrections and gap decisions,
actual active human time, annotator and editing tool. Use the same workflow and
order-balanced comparison for both backends. Frozen absolute caps are five beat
edits per 100 reference beats, one bar edit per 100 reference bars and 60 seconds
active correction per minute of audio.

A corrected-legacy comparison, when measured, must show no regression on critical
downbeats and at least 20% fewer total operations and active time when the legacy
baseline is nonzero; preserve zero when the baseline is zero. The known legacy
downbeat units bug must be repaired before obtaining that comparator. It is not
a prerequisite to installing/running the selected model. Without paired human
measurements, reduced correction burden remains unproven. The absolute v1 caps
establish preliminary editing eligibility only; they do not complete the required
replacement acceptance or establish comparative improvement.

## Resource and publication gates

| Property | Frozen limit |
| --- | --- |
| Native staging and complete worker PCM | 512 MiB each |
| Worker response / final diagnostic envelope | 8 MiB / 1 MiB |
| Logits per channel | 250000 |
| Complete job wall time | <= min(120 s, 15 s + 0.15 * loaded duration) |
| Actual model-process peak working set | <= 1.5 GiB |
| Sampled simultaneous parent/worker working set | <= 4 GiB |
| Selected optional installation logical size | <= 4 GiB |

Every frozen complete track must publish its full raw predictions. Explicit
admission or publication rejection is correct limit handling and still a failed
acceptance case. Do not truncate, drop logits or relax limits to make it pass.
Keep raw worker output before the final envelope merge so an oversize rejection
does not erase diagnostic evidence.

The native staging formula is
`4*N*(channels+2) + 4*ceil(N*44100/rate) + 1 MiB`, for loaded frame count N.
It counts the retained source and PCM copies, excludes FFT/CQT/ORT/model workspace,
and is not an RSS guarantee. At 96 kHz stereo its admission ceiling is approximately
312.9 seconds, before the resampler's padded-allocation checks. Do not present
a 48-kHz file as a 48-kHz loaded source when the device actually runs at 96 kHz.

The 120-second worker timeout excludes preflight/startup, native key work and
retirement. A running key call has no hard cancellation deadline; actual resource
retirement and subsequent admission must be observed. Worker count/thread limits
do not impose an OS memory limit.

Installation logical bytes exclude shared uv/base-Python caches, separate developer
environments and other installations, which must be reported separately. CPU FP32
does not allocate model CUDA VRAM. First/repeat runs start fresh model processes;
they are not controlled cold-disk or persistent warm-model benchmarks.

Simultaneous RSS must sum successfully sampled live processes only. The B1b
monitor retained last RSS after exit; its published combined figures are provisional
and must not be used to pass this gate. Per-process recorded peaks remain separate.
Muted clock/playhead/ping telemetry does not establish callback-duration percentiles,
driver underruns, acoustic synchronization, multi-pad behavior or B8 acceptance.

## Measured resource decision

The 2026-10-05 resource gate **fails**. Musical acceptance remains **pending**.
The complete local export is `exports/b2a-acceptance-20261005.json`; individual
raw worker responses, pre-limit envelopes, process samples and sealed identities
remain in `scratch/b2a/`. No acceptance limit or selected model was changed.

Complete native jobs used actual 96-kHz stereo loaded PCM. These cases retained
both full logit arrays, published once, retired their worker resources and allowed
subsequent native admission. H01/H02 are supplementary human-session sources,
not additions to the frozen held-out split.

| ID | Loaded duration (s) | Complete job (s) | Final bytes | Model peak (MiB) | Sampled live combined peak (MiB) |
| --- | --- | --- | --- | --- | --- |
| T01 | 241.006 | 17.825 | 502750 | 629.6 | 1297.5 |
| T02 | 170.631 | 12.918 | 355331 | 579.8 | 1091.1 |
| T03, after key fix | 212.402 | 16.428 | 435234 | 603.4 | 1166.7 |
| H01 | 108.983 | 10.042 | 227620 | 485.6 | 916.1 |
| H02 | 42.109 | 6.450 | 88608 | 482.3 | 830.5 |

T04/T05/R01 were rejected before native inference by the unchanged 536870912-byte
staging cap. Calculated staging was respectively 613699172, 888980404 and
1029394420 bytes, at loaded durations 357.773, 518.531 and 600.529 seconds.
No truncated native success is reported.

Separately labeled worker-only runs decoded each complete original source to
44.1-kHz mono and exercised the configured worker and real final-envelope
serializer. They isolate inference/publication from the failed native admission;
they do not establish native PCM parity, native key completion or a complete job
time. All complete logit arrays and raw responses were preserved.

| ID | Adapter time (s) | Worker response bytes | Unlimited final bytes | Actual final publication |
| --- | --- | --- | --- | --- |
| T04 | 24.189 | 707289 | 744349 | Full ready result |
| T05 | 32.676 | 1024648 | 1078513 | Oversize failure, 995 bytes |
| R01 | 37.419 | 1193626 | 1254142 | Oversize failure, 995 bytes |

Their model peaks were 747.7/912.6/997.7 MiB and sampled live combined peaks
804.8/969.2/1053.8 MiB. Worker PCM, response, logit and model-memory limits passed;
T05/R01 full final envelopes exceeded 1 MiB. Complete frontend coverage is not
evidence of correct musical beat counts. A constructed serializer boundary probe
crossed the cap at 25117 logits per channel, but JSON values and event counts vary;
it is not a universal duration ceiling.

The selected optional installation occupies 3362965600 logical bytes across
17042 files, including the 81058141-byte checkpoint. It passes the frozen 4-GiB
gate with the exclusions above. Native staging and final publication each require
bounded remediation before B2 can pass. Address full, lossless publication first
as a separate slice, then native long-track staging; preserve the frozen limits,
corpus and raw evidence throughout.

## Native key completion correction

The original T03 run produced a ready Beat This response but could not retire
its native reservation: KeyNet emitted valid `G#m`, while native validation only
accepted its legacy flat spelling `Abm`. A bounded diagnostic reproduced
`invalid ready key component`; its forced cleanup is not a successful analysis.
The initial 300-second harness timeout remains a failed pre-fix case.

Validation now uses the authoritative producer's 24 key names and retains all six
legacy flat aliases, preserving published spelling. The locked-release T03 rerun
published ready beat/key components with `G#m`, emitted one completion event,
retired naturally and allowed a new reservation. This corrects result acceptance;
it does not alter the model, musical key computation or default routing.
The pre/post-fix T03 raw worker responses are byte-identical: 413085 bytes,
SHA-256 `5611dd61bded4c14c95c28a8a3bbb33fabd51d95f284ded7200300cfeb017d91`.

T01's first attempt is excluded because the inherited harness incorrectly opened
an MP3 with the WAV-only reference reader after inference. The corrected, separately
retained full retry supplies the reported T01 measurements.

## Current musical decision and initial-marker feature

The corpus and criteria are frozen; independent full-span labels and correction
sessions remain pending. The user completed a separate release-app session and
manually shifted two supplementary tracks' grids slightly right, then aligned
their loop starts to the chosen grid lines. The sealed local supplement is
`scratch/b2a/human-anchors-supplement.json`, SHA-256
`1b40532079041e717aa81902da9ec0b58e4d0ee84c8330a4a4d7fc261983ea3d`.
Their legacy BPM values remain 93.98298645 and 94.01548004, with no manual BPM
override. These are sparse human-approved start/grid intents, not independently
verified full-span beat/downbeat labels or additional held-out recordings.

The user subsequently requested automatic first-waveform loop placement on new
loads. That separate [change](../openspec/changes/auto-place-new-track-loop-start/proposal.md)
does not reinterpret the manually verified grids or complete Beat This acceptance.
The locked-release controller smoke restored both approved grids, fractional BPMs
and manual loops unchanged. A new stereo fixture with one second of blank audio,
then anti-phase signal, initialized its loop at 0.994927083 seconds (loaded frame
95513 at 96 kHz), with bounded 5-ms pre-roll, Auto 8 bars and unchanged grid origin.
The first signal can be a pickup, sample or noise; it is not a certified downbeat.

Full validation passed: environment sync, debug and locked-release builds, cargo
check, 462 Rust tests (one ignored doc test), 921 application Python tests,
Ruff/mypy, formatting/whitespace and official strict validation of
`auto-place-new-track-loop-start`, `adopt-beat-this-analysis` and
`share-editor-beatmap-coordinates`. The installed release SHA-256 is
`c025b50b2c263872fefd4f98322200a2e154c1126df313eec67560a7c65d8cd1`;
earlier B2 native measurements used the original frozen B1b release except for
the explicitly labeled post-fix T03 rerun.

No new default, saved-result adoption, live map/SYNC behavior or production pitch
intent is enabled by these measurement and initial-marker changes.
