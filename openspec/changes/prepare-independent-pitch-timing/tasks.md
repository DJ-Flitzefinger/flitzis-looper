## Contract preparation

- [ ] Freeze canonical frame/beat units, independent semitone intent and exact render equations.
- [ ] Document current clamp/bypass limits and requested/pending/effective/rejected semantics.
- [ ] Define prepared-state and derived-cache pitch identity without changing raw source identity.
- [ ] Keep production k=0; expose nonzero k only in isolated tests/examples for this slice.

## Diagnostics

- [ ] Add independent source/master/target oracles with nonzero test-only k and no live adoption.
- [ ] Cover equal-rate h/r versus q=1/r,p=h paths and proposed KEYLOCK-off p=h behavior.
- [x] Establish the first bounded original-source comparison on the existing
  48-kHz/r2/H16384 impulse: predeclared offline R2/R3, q=.5,p=1, explicit geometry
  and total duration, complete uncropped/strict-target fixed metrics and processing
  partition comparison. Retain every negative and all wider parent obligations.
- [x] Preserve direct-native partition failures and prove a bounded fixed-native-
  block offline adapter against both caller partitions and the complete direct
  fixed-block PCM, with identical source/map and zero fitted tolerance.
- [x] Evaluate one frozen causal AR2 source-filter candidate on the original
  impulse, sustained mono and mixed two-tone sources, identity and nonzero pitch;
  preserve wrong-frequency residual forcing as rejection rather than impulse-only acceptance.
- [x] Evaluate one frozen source-segmented full-PCM overlap candidate with source-only
  cues, complete retained grain tails, identity/calibration, short and mixed tones,
  nearby attacks and a paired running-background control; preserve all negatives.
- [x] Evaluate a frozen full-phase-gradient candidate with centered-coordinate
  integrity, complete signed packet/SRC tails, original short-attack/lower-polyphony
  failures, matched busy control and unchanged acoustic criteria.
- [x] Evaluate a frozen complete finite subband AM/FM candidate, transforming every
  component and retaining all tails; distinguish analysis identity from short-attack,
  nearby-pair, independently gated mixed-carrier and busy-background quality.
- [x] Calibrate the unchanged finite bank's positive/negative image responses,
  onset winding, transformed synthesis gain and inverse conditioning against
  actual PCM; retain every out-of-span component and original acceptance gate.
- [x] Evaluate source-derived independent stationary modes/envelopes plus a fully
  transformed exact residual on original and unseen sources, preserving mode
  birth gauges, full tails, fractional identity and unsupported-domain reports.
- [x] Compare source-only compact/spread-column shadow certificates after energy
  refusal without changing carrier admission, the renderer or acoustic gates.
- [x] Evaluate certified stationary modes with trailing/left-held local envelopes,
  fixed-basis prefix controls and the unchanged original/unseen full-response matrix.
- [x] Evaluate causal affine endpoint envelopes with actual-design conditioning,
  independent source controls and the unchanged complete-response quality matrix.
- [x] Evaluate projected affine-slope extrapolation between fixed source knots,
  with independent full-hop, zero-crossing and prediction-sensitivity controls.
- [x] Evaluate positive current weighting with independent signed source-knot
  responses and distinct weighted-target/PCM sensitivity before full quality.
- [ ] Verify inverse-map seconds to loaded-frame conversion before deriving rate; equal physical
  progression at equal sample rates must yield r=1, including across supported device rates.
- [ ] Test all37 extras/base -5..+6/total -23..+24 at actual r(n), lockON h/r
  (~.132433..8 for r=.5..2), lockOFF h, original .25..4 corners and finite bounds
  without clipping; prove quality/RT/readiness/latency/finite context/unity transitions.
- [ ] Test unit-rate transposition, p=1 crossings, prepared old/new output alignment and native history.
- [ ] Test pending launch/KEY revisions, late preparation, cancellation, queue failure and stale state;
  distinguish superseded prepare-work from distinct admitted attacks with frozen tuples/permits,
  no early quantized retune, no accepted repeat coalescing and no removed-lifetime execution.
- [ ] Compare partitions, variable maps, loop seams, stem masks and isolated edits among 1/4/8 pads.
- [ ] Export logical invariants and separate pitch/acoustic/resource failures with original gates intact.

## Acceptance

- [ ] Run full required native/Python checks when implementing this shared audio diagnostic boundary.
- [ ] Run `openspec validate prepare-independent-pitch-timing --strict`.
- [ ] Verify no source/runtime behavior, control, original audio, map, global speed or manual key changed.
- [ ] Record measured limits and prerequisites for a separate future KEY UI/persistence/input proposal.
