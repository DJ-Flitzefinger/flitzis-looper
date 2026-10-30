## Current bounded evidence

The existing offline owner and initial comparison against an actual SourceBeatmap
are implemented. See [historical accepted-map-render-comparison.md](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/accepted-map-render-comparison.md) and the
`compare-accepted-map-renderers` change for measured results and limits. These
bounded diagnostics do not complete the wider matrix or its acceptance tasks.

## Prerequisites

- [ ] Confirm exact map units/quality and the experiment's native API/version/options.
- [ ] Freeze fixture identity, original failed gates and separate new diagnostic budgets.

## Experiment

- [x] Add an isolated offline Stretcher keyframe adapter with explicit endpoints and duration ratio.
- [ ] Compare it with current source-read/LiveShifter processing using identical exact maps.
- [ ] Measure unsmoothed versus anchor-integral-preserving rate trajectories and bounds.
- [ ] Inject independent semitone trajectories and measure pitch plus timing at combined range/unity-crossing cases.
- [ ] Prove identical canonical timing across k values and reject unsupported pitch/rate pairs without hidden clipping.
- [ ] Test pitch-revision cancellation, source-specific preparation and same-output-range handovers.
- [ ] Test stereo/full-mix/stem timing, mask changes, wraps, seams, source sums and cold starts.
- [ ] Export provenance, expected/observed positions, original gates, resource costs and failures.
- [ ] Write a measured render recommendation with unresolved live preparation/causality limits.

## Acceptance

- [ ] Run full native/Python project checks and strict validation for this change.
- [ ] Confirm no live mode or previous threshold changed and no production result is claimed.
- [ ] Define a separate live adoption proposal only after the experiment passes its stated gates.
