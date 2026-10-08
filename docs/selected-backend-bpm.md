# Complete selected-backend BPM metadata

The bounded B2 implementation derives versioned, unverified BPM metadata from
complete Beat This results. It uses G2's existing native numerical fitter and
keeps complete-track estimates, local intervals and regional estimates separate.
It does not change the automatic analyzer default. Full independent reference,
musical/count accuracy, paired human correction and human/device gates remain open.

## Frozen policies

`selected-backend-bpm-v1` retains every original binary64 timestamp, all four
prediction arrays, validated request/model identity and complete loaded rate,
frame count and source zero. The metadata identity is the existing request token;
it is not a new original/PCM content seal or current G3 authority.

The default coordinate assigns ordinal `i` to detection `i`, explicitly assuming
one quarter note per detection. This is an unverified interpretation. Half and
double interpretations remain visible and are never chosen by integer proximity,
downbeat cadence, model votes or interval-ratio guesses. The explicit-count API
instead accepts a full rational map: numerator/denominator, denominator 1..64,
positive count jumps for missing events and `None` for excluded extras. Supplied
counts and their provenance remain unverified here. Independently validated counts
still need the separate G2 evidence/acceptance path. Snare anchors spanning four
quarters must use that count, rather than assuming one quarter between snares.

The complete estimate is centered least squares on **all assigned observations**,
with equal observation weights. No cropping, robust exclusion or region choice
changes this descriptive estimate. Residuals retain the complete raw-index extent.
The existing robust `constant-period-candidate-v1` assessment runs separately on
the same counts and complete source duration, with its unchanged middle seed,
complete refits, temporal thirds, exclusions, coverage and affine-feasibility rules.
A robust fit remains visible even when its distant/coverage checks reject constant
tempo. A full OLS estimate is still descriptive if constant support fails.

The conditional lattice halfwidth is 0.01 seconds for Beat This's 20-ms output
lattice. It describes quantization **conditional on correctly detected events**,
not acoustic timing or count accuracy. The report states
`detector-lattice-only; musical timing/count unverified`. Conditional slope
sensitivity is `halfwidth * sum(abs(centered_counts)) / sum(centered_counts²)`
in quarter units; residuals, numerical floor and distant-period spread are separate.
This is not a confidence interval and cannot supply a G3 `TimingBound`.
Correlated errors do not become independent through repetition.

Adjacent raw intervals and count-conditioned local BPM remain available, including
`None` where a count is excluded. Fractions are binary64 throughout; no integer
rounding establishes musical truth. Fitted intercepts are diagnostics and never
replace source zero, a signed grid origin, loop start or a manual/TAP offset.

`representative-middle-region-v1` freezes four source-time windows before private
evaluation: middle `[0.2D,0.8D)`, early `[0,D/3)`, central `[D/3,2D/3)` and late
`[2D/3,D)`, where `D` is complete loaded duration. Full inference is never cropped.
Each regional fit reuses G2's robust fit and affine-feasibility check, retaining
original counts and raw indices. It requires:

- At least 24 assigned/retained observations and at least 30 seconds of retained span.
- Retained support starting within the first 20% and reaching the last 20% of its
  window, with at least 60% window-duration span.
- At most 10% raw exclusions and at most two consecutive raw exclusions within
  the window; explicit `None` counts consume those allowances.
- A robust threshold below a quarter of the quarter-note period and a feasible
  affine model within the conditional lattice bound plus reported numerical floor.

Each candidate remains present even when unsupported. Eligible middle wins;
otherwise the eligible third with longest retained span wins, then most inliers,
then earliest source window. No eligible region produces explicit unavailable
representative metadata. Short sources are not silently given a short-window policy.
Regional refits give equal weight to every retained observation; selected-region
metadata never overwrites complete OLS or complete G2 status. A stable middle with
sparse/unsupported edges is scoped unverified metadata, not full-source constant
timing. An ambiguous middle may yield a supported distant third while complete
timing remains unsupported. These policies are frozen engineering choices, not
thresholds tuned against held-out musical labels.

## Integration and bounds

The pure `summarize_selected_bpm_json` binding detaches native fitting from Python,
runs no model/device/file operation and changes no engine owner. Python
`assess_beat_sequence` validates the typed assessment and exact raw binary64 parity.
`summarize_beats` retains full immutable prediction/request metadata.
`summarize_published` explicitly derives it after the existing v1/v2 envelope reader
validates the complete bounded publication. Ordinary reference-first candidate and
seal readers keep their native-free paths. Historical envelopes and worker bytes
are unchanged.

`OfflineAnalysisService` computes metadata on its background supervisor after
component/PCM retirement and from the actual bounded final envelope. It exposes
`JobSnapshot.bpm_summary` only after native `finish` accepts the same request.
Cancellation, superseded source, finish refusal, unavailable beats and oversize
publication expose no summary. Numerical metadata errors remain explicit in
`bpm_summary_error`; they cannot strand admission or overwrite full beats/key.
Independent key success/failure remains independent.

There is no ProjectState, saved-analysis, manual/TAP, accepted-period, controller
timing or live map adoption in this slice. Existing precedence remains manual/TAP,
current native accepted timing, then legacy metadata. A region cannot silently
replace full timing. Accepted period P, physical integer H, one rate owner, native
freshness, leases and old voice/stem/FIFO/filter/seam ownership stay unchanged.

Work is bounded by 250,000 positions, at most four regional candidates and G2's
65 seed positions/three refits/64 feasibility iterations. Source frames are
positive and at most 2^53; loaded rate is 8,000..768,000 Hz. Numerical metadata
JSON has a separate explicit 64-MiB cap and never truncates arrays. Existing worker
8-MiB and final 1-MiB envelopes, PCM budgets and native admission bounds remain
unchanged. Metadata duplicates numerical vectors off realtime; this slice makes
no new process-RSS, callback, latency or acoustic acceptance claim.

## Evaluation boundary

Regressions cover exact 120 and true 119.999/123.45, quantization, distant drift,
local tempo changes, sparse edges, an ambiguous middle, missing/extra events,
explicit snare counts, invalid input and immutable publication/intent ownership.
The actual private evaluation reuses all five approved full native candidates and
the retained unchanged 600-second G2 source/worker bridge. Candidate outputs are
never reference truth; no full ReferenceSeal exists. Numerical fits and reported
uncertainty are engineering evidence only. The metronome has 1200 pulses from
0 to 599.5 seconds, 1199 quarters, and no pulse at 600 seconds; extrapolated
slope and measured pulse-span slope remain distinct from worst-case sensitivity.

See [G2 count/evidence contracts](constant-tempo-summary.md),
[G3 ownership](native-constant-timing.md),
[scoring workflow](beat-this-scoring-workflow.md) and
[remaining migration gates](beatmap-sync-design.md).

The frozen policy's actual full-candidate numerical evaluation reports:

| Source | Complete ordinal OLS BPM | Global constant evidence | Eligible longer region |
| --- | ---: | --- | --- |
| T01 | 94.00688629170699 | Unsupported | None |
| T02 | 105.27111262134812 | Unsupported | None |
| T03 | 132.41606814914152 | Unsupported | None |
| T04 | 100.79716371658623 | Unsupported | None |
| T05 | 146.3231089745694 | Unsupported | None |

These are complete prediction summaries, not true musical BPM or diagnoses of
why particular detections disagree. The conservative constant/region gates were
not relaxed to make these candidates pass. Every candidate's complete OLS matches
an independent exact-rational calculation from its retained binary64 timestamps;
all four arrays and prior failed v1 chains remain unchanged. Private full reports
retain every rejection, candidate binding and local interval.

On the actual independently byte-checked 600-second source, all 28.8M decoded
PCM24 frames equal the retained native mono. Complete, robust and representative
metadata yield exactly 120 BPM; measured 0..599.5-second and separate 600-second
extrapolated slope errors are zero. The new **raw detector-lattice** conditional
OLS sensitivity over the measured span is 1438.8009991673605 loaded frames. The
earlier G2 **refined discrete-feature half-frame** sensitivity of about 1.498751
frames is different evidence and stays preserved. Zero measured error does not
turn either bound into a universal one-frame proof. General musical tempo, all
five independent full-span references, scoring and default adoption remain open.

Hardware-free native T03 probes in both build profiles separately exercise actual
copy-first/complete-source preparation, frozen CPU worker, independent KeyNet,
retirement, native finish and next admission with the new full metadata snapshot.
The executing source-bridge test EXE and pure-fitter imported PYD are separately
resolved and retained. These probes add no device, GUI or human listening evidence.
Reproduction and final source/log/runtime/review identities stay private under
`scratch/b2-bpm-regions-20261008/`; no candidate or reference audio is committed.
