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
- [ ] Test combined 0.25..4 pitch corners, unsupported bounds and finite-value validation without clipping.
- [ ] Test unit-rate transposition, p=1 crossings, prepared old/new output alignment and native history.
- [ ] Test pending launch/KEY revisions, late preparation, cancellation, queue failure and stale state.
- [ ] Compare partitions, variable maps, loop seams, stem masks and isolated edits among 1/4/8 pads.
- [ ] Export logical invariants and separate pitch/acoustic/resource failures with original gates intact.

## Acceptance

- [ ] Run full required native/Python checks when implementing this shared audio diagnostic boundary.
- [ ] Run `openspec validate prepare-independent-pitch-timing --strict`.
- [ ] Verify no source/runtime behavior, control, original audio, map, global speed or manual key changed.
- [ ] Record measured limits and prerequisites for a separate future KEY UI/persistence/input proposal.
