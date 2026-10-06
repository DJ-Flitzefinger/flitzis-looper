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
G2b2 must bind that distinction to the existing immutable PCM/job identity and
the already lossless Beat This reader before count/refinement assessment.

Pure core tests do not establish signal refinement, normal pad-load correction,
frozen musical acceptance or audible synchronization. Docs and handoff must keep
the remaining G2b and G3 work explicit.
