# Offline constant-tempo candidates

G2a adds `flitzis_looper_analysis::tempo_summary::summarize_constant_tempo`.
This pure Rust API assesses explicit count hypotheses against complete binary64
timestamps. G2b adds the offline `tempo_evidence` binding/adapters and narrow
`tempo_refinement` PCM feature assessment. These APIs do not update pads or publish
BPM. The existing automatic 120.00128936767578-BPM reference result therefore
remains unchanged until the separate G3 adoption stage.

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
The G2b2 binding retains this analyzer domain separately from loaded source
evidence. The existing Beat This `decode_result` reader retains all four complete
binary64 prediction arrays and request/model identity; the offline adapter keeps
that complete evidence beside the summary.

## Content binding and adapter scope

`tempo_evidence` hashes the complete borrowed loaded mono float32 PCM in
little-endian order. The caller supplies an independently established original
source SHA-256 with provenance and an immutable source-generation/request token.
The result retains source zero, loaded rate and complete frame count. A path,
generation token or matching duration is not a content hash.

| Retained domain | Evidence |
| --- | --- |
| Original source | Asserted content SHA-256 and independent establishment provenance. |
| Immutable loaded mono PCM | Actual complete sample-content SHA-256, loaded rate/frame count and original source zero. |
| QM analyzer input | Actual complete input-content hash, input rate/frame count, integer hop and versioned loaded-to-input transform. |
| QM raw result | Every original binary64 detector frame, downbeat raw index and requested configuration field. |
| Beat This raw result | All binary64 beats/downbeats/logits, full model/configuration and expected request identity. |

Raw revision hashes include complete backend evidence and configuration/timebase,
preserving binary64 bits. Adapters neither sort, crop nor deduplicate arrays.
Beat This downbeats stay independent; the worker protocol does not require a
beat-index subset association. Analyzer-input dimensions never replace loaded
PCM dimensions. Origin and the fitted intercept remain distinct.

The adapter checks identity assertions and PCM content, not the historical truth
of the original-file-to-loaded-buffer relationship. Current loading decodes
before copying the original. This slice has no production load/job caller and
does not establish the planned copy-first source-byte ownership or PCM cache.
The existing immutable snapshot/job retirement boundary remains authoritative
when a later caller adopts the API.

`PcmBinding::verify` checks the actual samples against `PcmBindingMetadata`.
`BoundTempoEvidence::from_qm` takes the retained capture, actual complete binary64
analyzer input, `QmInputDescriptor` and explicit `TimingBound` with provenance.
An identity transform checks exact float32-to-float64 promotion. The declared
Rubato 44.1-kHz transform checks the complete ceiling-derived frame count and
input hash; its actual execution lineage remains a caller assertion. The
adapter does not rerun QM or reproduce resampler samples to prove that assertion.
`BoundTempoEvidence::from_beat_this` checks full `BeatThisRawEvidence` against
the complete expected request and echoed identity/model. `backend()`, `binding()`
and `timing_bound()` retain evidence omitted from the core's `raw_evidence()`.
This checks consistency with the supplied retained job, not the current engine
pad state. G3 adoption must separately reject a source/request that has become
stale before publication.

## Comparable PCM features and count support

`tempo_refinement` supports `isolated-comparable-attack-v1`, an intentionally
narrow discrete signal feature policy. It scans the complete loaded source
without a predicted tempo grid. An attack has absolute amplitude above 0.01,
at least 5 ms of below-threshold separation and at most 100 ms active extent.
Every active cluster, from its first to last above-threshold frame, must have
the same complete float32-bit shape. Incomparable,
colliding or insufficiently isolated attacks remain unsupported.

The feature frame is the first above-threshold loaded frame. Frame zero is
allowed with an explicit boundary-attack flag because preceding silence was
not observed. Insufficient trailing silence rejects support. The half-frame
positioning bound is conditional on this discrete threshold feature; it is
not a universal acoustic-onset or musical-beat accuracy claim.

A refined raw detector position must map uniquely to a feature within a bounded
100-ms search. Unassociated raw extras remain explicit exclusions. The result
retains original positions, refined feature frames,
raw-index mapping, displacement, unmatched features and policy/provenance.
A source-zero attack omitted by QM remains in the separate complete feature
sequence; it is never inserted into the immutable QM raw arrays.

Ordinal, half-tempo and double-tempo proposals are all `Unverified`. Repeated
attacks establish comparable signal features, not musical quarter-note units.
An `IndependentQuarterEvidence` record supplies source/PCM digests, the exact
loaded rate/frame count, feature policy version, complete feature-frame sequence,
explicit rational counts and provenance.
Its matching independent assertion is the only verified-count path. The
validator checks identity and extent; it cannot prove the assertion's musical
truth or derive it from fit quality. Missing/extra events and distinct count
interpretations retain their explicit mappings and ambiguity.

Original raw-evidence and complete-attack summaries are separate. The latter
uses the same multiple-position and distant-region core once its counts are
independently supplied. No integer-nearness heuristic selects a BPM. All
hashing, scans, allocations, searches and fits remain outside realtime paths.

`refine_comparable_attacks` returns `TempoRefinement` with a status and explicit
rejection reasons. Only `ComparableAttacks` supplies supported signal features;
`Unsupported` preserves the original source revision/uncertainty, supplies no
feature positioning bound and returns no count proposals. `CountProposal`
provides separate `as_raw_hypothesis()` and `as_attack_hypothesis()` mappings for
the G2a fitter. Keep the complete backend binding, refinement and both summaries
together; a `TempoSummary` alone does not retain all QM or Beat This evidence.
The PCM policy caps input at 512 MiB, attacks/raw positions at 250,000 and
search halfwidth at 100 ms. The conditional feature bound applies only when
the comparability policy passes.

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
.\scripts\run-rust-tests.ps1 -CargoArgs @('--package', 'flitzis-looper-analysis', '--test', 'tempo_summary')
.\scripts\run-rust-tests.ps1 -CargoArgs @('--package', 'flitzis-looper-analysis', '--test', 'tempo_evidence')
.\scripts\run-rust-tests.ps1 -CargoArgs @('--package', 'flitzis-looper-analysis', '--test', 'tempo_refinement')
```

G2b1 preserves QM frames before binary32 projection and tests exact legacy parity,
long-position precision, downbeat associations, requested/actual timebase and
short/invalid input behavior. This capture is not a source-bound adapter or signal gate.
G2b2 adds content-bound adapters using QM capture and complete Beat This binary64
evidence, unverified count proposals and isolated comparable-attack refinement.
Adapter tests cover identity/timebase mismatches and complete-array retention;
PCM fixtures cover true fractional periods, varying tempo, collisions,
incomparable attacks and count assertion failures separately. Independent count
evidence also binds rate/frame count and feature policy; identical PCM bytes
interpreted at another rate cannot reuse a verified assertion.

The actual private 48-kHz reference probe uses complete verified native loaded
PCM. QM consumes the existing native complete Rubato 44.1-kHz conversion before
lossless capture. The retained actual Beat This result passes through the
existing envelope encoder/reader with bit-exact retention of all four binary64
arrays before its adapter; no new inference session is required.

Both adapters support all 1200 comparable PCM attacks and 1199 independently
asserted quarter intervals. The complete-attack fit yields exactly 0.5 seconds
per quarter, or 120 BPM, with zero loaded-frame slope error over the measured
0..599.5-second pulse span. The separate 600-second grid extrapolation also has
zero slope error. QM's 1199 original raw events map to attacks 1..1199, leaving
attack zero explicitly unmatched; Beat This maps all 1200. Without the independent
quarter evidence, the half/normal/double proposals remain ambiguous.

The conditional fit sensitivity over the measured span is about 1.498751 loaded
frames and is retained separately. Empirical zero slope error passes the
one-frame fixture gate; it does not claim a worst-case uncertainty of at most
one frame. The discrete-feature bound and quarter-unit assertion remain explicit.
Legacy QM still returns 120.00128936767578 BPM. This probe proves neither general
music refinement, automatic pad-load repair nor audible synchronization.

G3a now provides an immutable explicitly accepted period/revision and a
control-only adoption guard; see [accepted timing](accepted-constant-timing.md).
It recomputes this evidence path. G3b2a/b/c connects actual current-pad validity
and explicit native accepted publication to native/Python grid, loop and
transport/global controls; see [native adoption](native-constant-timing.md).
Full-revision MIDI/preparation/persistence consumers remain pending; G3c separately
tests musical versus physical loop duration. Default analyzer acceptance,
variable maps and audible synchronization retain their separate gates. See
[the diagnosis](grid-timing-diagnosis.md), [scalar coordinates](scalar-source-coordinates.md)
and [the migration design](beatmap-sync-design.md).
