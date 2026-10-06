# G2 boundaries and design

## Evidence before publication

`flitzis_looper_analysis::tempo_summary` owns a pure offline assessment. Complete
binary64 raw positions, source/PCM identity, source coordinate domain, backend
configuration/raw revision and declared position-error bound are inputs. Count
hypotheses refer back to every raw index, recording explicit exclusions and
quarter-note interpretation provenance. Raw arrays and the independent grid
origin are immutable; fitted intercepts are diagnostics only.

G2a consumes explicit hypotheses rather than inventing musical counts from timing.
A missing event can advance by more than one quarter; an extra event can be
explicitly excluded. Rational count coordinates represent subdivisions as well
as whole quarters, so a half-tempo interpretation does not discard every other
raw event. Equal residuals cannot resolve half/double tempo. Unknown
unit provenance and distinct viable count interpretations prevent a supported
candidate. This is intentionally stricter than selecting the nearest integer BPM.

## Fit and uncertainty

Freeze the numerical region/fit policy under a version identifier. Seed a bounded
robust multiple-position fit from complete and middle evidence, then assess full
coverage, early/middle/late slopes and residual offsets. Middle position is a
preference, not proof. Cap outlier fraction and contiguous rejection runs so a
stable island cannot conceal a count step or changing outro. Retain every
residual, exclusion and region decision for review. An explicit bounded affine
feasibility check prevents alternating variation outside the declared timing
bound from passing only because it fits a wider robust seed threshold.

Use centered binary64 arithmetic. A declared position-error bound propagates
through the linear slope weights; it is conditional on correct counts and a
constant model. Empirical residuals/window spread remain separate. Do not call
this a calibrated statistical confidence interval or reduce detector resolution
by assuming independent random errors. Finite event/hypothesis/sample/iteration
limits bound work and avoid an all-pairs search over complete input.

## Stages

1. G2a: implement and adversarially test the standalone evidence/count/fit core.
   Existing QM BPM and Beat This publication remain unchanged.
2. G2b: capture QM frame times before binary32 conversion and reuse complete
   Beat This binary64 evidence. Establish count proposals and support without
   treating predictions as independent musical labels. Add comparable-attack
   PCM refinement with original/refined positions and versioned provenance.
3. Validate the actual private exact WAV: all 1200 pulses and verified 1199
   quarter intervals over 0..599.5 seconds at 48 kHz support at most one loaded
   frame of period-slope error. Report the 600-second extrapolation separately.
   Fractional, count-ambiguous and variable-tempo fixtures are separate gates.
4. G3 separately adopts one source-bound accepted timing revision across control
   and live consumers and proves musical versus physical loop duration.

G2b1 isolates the first loss boundary: `analyze_bpm_raw` captures the complete QM
detector frames and downbeat raw indices with actual integer hop, input rate/count
and complete configuration. Binary64 seconds are derived on demand; ordinary
`analyze_bpm` projects the same capture directly to its existing binary32 output.
No second tracker/downbeat run or eager full binary64 seconds buffer is required.
This layer identifies the analyzer input, which may be resampled from a loaded
source. It cannot establish loaded-source/PCM digests or a musical timing bound.
G2b2 binds that distinction to immutable loaded PCM/job identity and the already
lossless Beat This evidence before count/refinement assessment.

## Source binding and complete backend evidence

The public offline `tempo_evidence` module hashes every complete borrowed loaded
mono float32 sample in little-endian order, preserving source zero and extent.
The caller supplies independently established original-source SHA-256 and its
provenance plus immutable source-generation/request identity. A generation token,
path, common filename or duration cannot substitute for a content digest.
The API validates these assertions and the actual PCM content, not the historical
truth of an original-file-to-loaded-buffer relationship. The current loader
decodes before copying; its copy-first source-byte guarantee remains a separate
future stage. No production load/job caller is added by this slice.

QM adapters retain the complete `QmRawAnalysis` beside the summary, including
binary64 detector frames, downbeat indices and every requested configuration
field. They separately identify and hash the actual complete analyzer input
and its loaded-to-analyzer transform, rate, frame count, delay/tail convention
and origin. Resampled 44,100-Hz input must not be described as loaded PCM.
Identity transforms check exact sample promotion. The standard Rubato transform
checks its actual input hash and complete ceiling-derived dimensions but retains
the caller's execution-lineage assertion; it does not rerun analysis/resampling
to prove that relationship.
Beat This adapters retain all four binary64 arrays, full model/configuration
identity and the expected request/source generation; they do not collapse
downbeats or logits into the tempo summary. Raw revision hashing includes all
retained backend evidence and timebase/configuration, preserving binary64 bits.
Neither adapter sorts, crops, deduplicates or moves the original origin.
Retained request consistency is distinct from current engine-pad validity. The
typed adapter cannot query current pad state; G3 must perform the runtime stale
source/request check before adopting timing.

## Explicit count proposals and independent evidence

Generated event-ordinal, half-tempo and double-tempo count interpretations are
bounded proposals, all `Unverified`. A timing gap, periodic PCM attack or small
fit residual cannot independently determine the musical quarter-note unit.
The existing rational coordinate/count mapping retains missing events, explicit
exclusions and distinct viable interpretations. Integer BPM proximity is unused.

Verified PCM count input requires an explicit independent evidence record with
matching source/PCM digests, loaded rate/frame count, feature policy version,
the complete exact discrete feature-frame sequence, one explicit rational count
per feature and provenance. This record is the
caller's assertion of independent quarter-note truth; structural validation or
the signal scan cannot establish its musical truth. A mismatching frame/count
extent or source identity rejects the input rather than repairing it from the fit.

## Conservative comparable-attack refinement

`tempo_refinement` freezes `isolated-comparable-attack-v1`: scan the complete
loaded source at an absolute amplitude threshold of 0.01, require at least 5 ms
of below-threshold separation and limit an active cluster to 100 ms. Every
cluster, from first to last above-threshold frame, must have the same complete
float32-bit shape. Source frame zero
may be an attack, with the missing preceding silence explicitly reported;
insufficient trailing silence and incomparable or ambiguous signals remain
unsupported. This intentionally narrow signal class is not a general onset
detector or universal music refinement method.

The measured feature is the first above-threshold loaded frame of each attack.
Its declared half-frame positioning bound is conditional on this discrete
feature; it is not a sub-sample acoustic or musical onset guarantee. Full-source
scanning is independent of a predicted tempo grid. A refined original raw position
must associate uniquely with one feature within at most 100 ms; unmatched raw
extras remain explicit exclusions. Preserve raw
positions, feature frames, raw-index mappings, explicit displacement, unmatched
attacks, search bounds and method/version. No feature is silently inserted into
the detector arrays. A source-zero attack missing from QM remains separately
available in the complete PCM feature sequence.

Keep the original raw-evidence summary and complete-attack summary separate.
Only matching independently asserted count evidence may give the attack summary
verified counts; it then uses the G2a multiple-position, coverage and distant
checks. The fitted intercept remains diagnostic and cannot replace the chosen
origin. All hashing, scans, allocations and fitting remain offline with bounded
dimensions and searches; there is no callback or live publication caller.
Unsupported refinement keeps the original source revision and timing bound,
has no feature positioning bound and supplies no count proposals.

## Actual reference evidence

The private exact-reference probe uses complete verified native 48-kHz PCM,
the existing complete Rubato 44.1-kHz QM input and retained actual Beat This
output with all four binary64 arrays preserved through envelope encode/decode.
Both source-bound paths retain all 1200 comparable attacks and 1199 independently
asserted quarter intervals. The complete-attack period is exactly 0.5 seconds:
zero loaded-frame slope error over measured 0..599.5 seconds and, separately,
zero error at the 600-second extrapolation. QM's 1199 original raw events retain
the unmatched source-zero attack separately; Beat This associates all 1200.
Generated half/normal/double proposals without independent quarter evidence
remain ambiguous. Legacy QM BPM remains 120.00128936767578.

The measured-span conditional sensitivity is about 1.498751 loaded frames.
The empirical one-frame slope gate passes with zero error; the conditional
sensitivity is not concealed or relabeled as a one-frame confidence bound.
No new app/device or inference session, general musical refinement acceptance
or automatic publication follows from this signal-specific probe.

Pure core tests do not substitute for actual-signal evidence, normal pad-load
correction, frozen musical acceptance or audible synchronization. The separate
G3 adoption boundary remains explicit.
