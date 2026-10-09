## Contract preparation

- [ ] Freeze canonical frame/beat units, independent semitone intent and exact render equations.
- [ ] Document current clamp/bypass limits and requested/pending/effective/rejected semantics.
- [ ] Define prepared-state and derived-cache pitch identity without changing raw source identity.
- [ ] Keep production k=0; expose nonzero k only in isolated tests/examples for this slice.

## Diagnostics

- [ ] Add independent source/master/target oracles with nonzero test-only k and no live adoption.
- [ ] Cover equal-rate h/r versus q=1/r,p=h paths and proposed KEYLOCK-off p=h behavior.
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
