# Offline constant-tempo candidates

G2a adds `flitzis_looper_analysis::tempo_summary::summarize_constant_tempo`.
This pure Rust API assesses explicit count hypotheses against complete binary64
timestamps. It does not run an analyzer, inspect PCM, update pads or publish BPM.
The existing automatic 120.00128936767578-BPM reference result therefore remains
unchanged until the later adapter/refinement and adoption stages.

## Lossless QM capture

G2b1 adds `flitzis_looper_analysis::analyze_bpm_raw` and `QmRawAnalysis`.
The shared QM pipeline retains every original binary64 detector-frame position,
downbeat raw-index association, actual integer sample hop, analyzer input rate
and complete input frame count. It also snapshots all requested `AnalysisConfig`
fields. That snapshot does not assert that previously unused tracker settings
have become active. Binary64 beat/downbeat seconds are derived from
`detector_frame * (actual_hop_samples / input_sample_rate_hz)` on demand.
Bars retain the legacy downbeat convention; no sort, crop, origin shift or
deduplication is introduced.

`beat_frames()` and `downbeat_raw_indices()` borrow the retained raw arrays.
`input_sample_rate_hz()`, `input_frame_count()` and `odf_hop_samples()` expose the
actual input dimensions; `configuration()` borrows the requested snapshot.
`beat_seconds()` and `downbeat_seconds()` return exact-size binary64 iterators.
`legacy_result()` produces the compatibility BPM/grid without another analysis run.

Ordinary `analyze_bpm` uses the same tracking result and preserves its existing
binary32 BPM/grid projection and arithmetic order. It neither runs the analyzer
twice nor creates an eager complete binary64 seconds buffer. Both entry points
retain the same short-input and invalid-timebase behavior.

QM capture describes the analyzer input. Normal loading resamples the loaded
source to 44,100 Hz before this call; that input rate/frame count must not be
misrepresented as loaded-source identity. Capture cannot establish source/PCM
hashes, a timing-error bound, independent grid origin or verified musical counts.
G2b2 must bind the retained input timebase/configuration to independently established
source evidence. The existing Beat This `decode_result` reader already retains
all four complete binary64 prediction arrays and request/model identity; it will
be reused when those backend-independent bindings are added.

## Evidence and count interpretation

`RawTempoEvidence` borrows complete source-relative beat seconds, `SourceIdentity`
and an independent signed grid origin. Identity includes source/PCM SHA-256,
loaded rate/frame count, backend/configuration/raw revision and a declared
per-position timing-error halfwidth in seconds. Callers must establish these
identities and the error bound; the fitter validates their shape, not their truth.

Every `QuarterNoteHypothesis` has an ID, provenance, verification state and one
optional integer count numerator for each raw position, with a shared positive
`quarter_note_denominator` up to 64. A musical coordinate is numerator divided by that scale:
`i/2` explicitly represents eighth-note observations at a half-tempo hypothesis.
A missing beat uses a count jump; `None` records an excluded extra event. Counts
are never reconstructed by counting array entries or rounding time ratios.
`Verified` is the caller's assertion of independent quarter-note evidence; a
good fit cannot supply it. A detector sequence alone is unverified.

The returned source-bound report preserves raw evidence, supplied count mappings,
all residuals/exclusions, the independent origin, selected seed region and policy
version. The fitted intercept is diagnostic. It cannot replace the origin or
change source coordinates, loop markers, saved data or manual/TAP intent.

## Decisions

| Status | Meaning |
| --- | --- |
| `Unsupported` | Adequate constant-period evidence was not established under this policy. |
| `Unverified` | Numerically viable count interpretation lacks independent quarter-note evidence. |
| `Ambiguous` | Distinct viable count interpretations remain, including half/double tempo. |
| `SupportedCandidate` | One numerically supported interpretation has supplied verification provenance. This is not accepted musical or runtime timing. |

Count maps differing only by an additive count origin or an equivalent rational
encoding are equivalent. Other
viable interpretations are not ranked by closeness to an integer BPM or by the
smallest residual. Invalid dimensions, identities, nonfinite/unordered times or
counts return `TempoSummaryError`, separately from valid unsupported evidence.

## Versioned engineering policy

`constant-period-candidate-v1` is a conservative diagnostic policy, not a tuned
musical acceptance threshold. It permits at most 250,000 raw positions and eight
hypotheses. It requires at least 24 supported positions, six supported observations in
each complete-source temporal third, and at least 60% source-duration coverage.
At most 10% of raw positions and at most two consecutive positions may be
excluded. Explicit exclusions count against these limits too.

Robust seed fits use complete and middle 20-80% source-region evidence, at most
65 seed positions (2080 pair slopes) and three centered least-squares refits.
The middle wins equal-support ties only. An observation enters the fit within
twice the declared position-error halfwidth plus an explicit numerical floor;
this threshold must be less than a quarter of the fitted quarter-note period.
The numerical floor is reported separately from the declared uncertainty.
Support must start within the first 20% and reach the last 20% of the source.

The candidate must additionally admit an affine timing model within the declared
position-error bounds. An alternating timing deviation cannot become supported
merely by falling inside the wider robust seed threshold. This feasibility check
uses at most 64 convex-search iterations over the conditional slope interval.
It tests existence of a compatible line; the returned period remains the
least-squares estimate, with its conditional sensitivity and residuals reported.

Individual residuals and distant periods and offsets remain available. Each
source third's independent local fit must retain at least 90% of its assigned
observations. Each third must also contain at least six observations retained
by the global fit, separately from the six required local inliers.
A stable middle cannot excuse unsupported edges,
a count step or changing source tempo. Sparse introductions/breakdowns may
therefore produce an unsupported result; this API does not certify constant
tempo over evidence gaps. Future policies require their own version and tests.

`period_sensitivity_bound_seconds` propagates the declared position-error
halfwidth through the centered linear-fit slope weights. For centered musical
quarter-note coordinates `x` (numerators divided by their denominator), the
sensitivity is `halfwidth * sum(abs(x)) / sum(x*x)`. This is conditional
on the supplied counts, selected inliers and constant model. It is not a
statistical confidence interval. Quantized or correlated detector errors do not
become independent random noise merely because many timestamps are available.
Residual maximum/range and distant-window period spread are separate diagnostics.

## Validation and remaining stages

External Rust tests exercise the public API against independent exact/fractional
fixtures, quantized detections, outliers, missing/extra events, conflicting counts,
tempo variation and invalid/unsupported evidence. Run on Windows with:

```powershell
.\scripts\run-rust-tests.ps1 -p flitzis-looper-analysis --test tempo_summary
```

G2b1 preserves QM frames before binary32 projection and tests exact legacy parity,
long-position precision, downbeat associations, requested/actual timebase and
short/invalid input behavior. This capture is not a source-bound adapter or signal gate.
G2b2 still needs identity-bound adapters using QM capture and existing complete
Beat This binary64 evidence, supported count proposals and conservative PCM
refinement of comparable attacks. The actual private 48-kHz reference must
support all 1200 pulses and 1199 quarter intervals with at most one loaded frame
of slope error over 0..599.5 seconds. Its 600-second extrapolation is separate.
Synthetic timestamps alone do not pass that signal gate.

G3 then publishes one source-bound accepted period/revision to all consumers and
tests musical versus physical loop duration. Default analyzer acceptance,
variable maps and audible synchronization retain their separate gates. See
[the diagnosis](grid-timing-diagnosis.md), [scalar coordinates](scalar-source-coordinates.md)
and [the migration design](beatmap-sync-design.md).
