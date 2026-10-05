## Experiment boundary

Draft only. Start with exact synthetic maps so detector error cannot be confused with rendering
error. Compare (A) mapped source reads plus pitch-only LiveShifter and (B) a separate offline
RubberBandStretcher adapter using source-to-output keyframes and total output/input duration.
Do not feed already warped audio through the same tempo change a second time.
Use the contract in prepare-independent-pitch-timing: h=2^(k/12), current varispeed pitch h/r
under KEYLOCK versus full Stretcher duration ratio 1/r and pitch h at equal rates. k is injected only
through the diagnostic/core boundary until a future user-facing KEY slice. Test original source
pitch plus k, not a compensator that always returns to original pitch.

Pin native version/build/options. Retain the existing R3 quality/FFT/resampler settings where
APIs are comparable; document unavoidable differences. Upstream keyframes are offline-only and
are not a guarantee about acoustic onset. Check start/end coordinate handling, omit a redundant
0-to-0 keyframe in the initial R3 experiment, set the complete map before processing and test
the tail/endpoint explicitly. Reset clears map state. Realtime Stretcher has different padding,
delay, retrieval and rate-change behavior; do not assume LiveShifter's delay applies to it.

For the mapped reader path derive source position from absolute master beat and inverse map.
Naively smoothing a succession of rates creates integral phase error. Compare raw piecewise
rates with a precomputed continuous positive trajectory whose segment integrals reach anchors.
Such smoothing must be a versioned shared B/S interpolation for all consumers, or be explicitly
reported as a noncanonical experiment. Preserving beat endpoints alone does not preserve
piecewise-affine source positions between anchors.
Reject trajectories that exceed supported rate/acceleration/headroom instead of silently
claiming sync after clipping. Keep initial read, future native feed and audible output separate.

For stems use one parent map, wrap/address trajectory, settings and state identity. Compare
independent stereo processors, a multichannel experiment and warp-once full-mix references.
CHANNELS_TOGETHER documents stereo behavior; do not infer linked eight-channel processing or
that summing independently warped stems equals the warped full mix. Separate separator error,
timing error, spectral differences and nonlinear null-test residuals.

Fixtures: drift, ramps, abrupt changes, swing, known markers, stereo correlation, short loops,
fractional BPM and existing causal marker histories at 44.1/48/96 kHz. Report cold/settled paths,
every original failing gate and exact continuation results. Repeated callback partition shapes
are offline simulations, not device deadline evidence.

Cross the full time/pitch matrix: r = 0.5/1/2 and variable trajectories; k = -12/-7/0/+7/+12 as stress
values; nonzero k at r = 1, h/r = 1 at nonneutral r, neutral crossings, live-style k ramps/steps,
different k across 1/4/8 voices, masks and KEYLOCK modes. The current inverse-pitch clamp 0.5..2 cannot
represent every combined case (possible h/r = 0.25..4). Do not silently clamp or promise that
extended range. Measure frequency/cents and timing independently and record unsupported pairs.
Prepared identity must include pitch trajectory/revision. A stale pitch state must fail adoption
without changing source phase or another pad. Raw analysis/stems remain valid after changing k.
Fixed-target offline caching is a reference candidate, not proof of responsive live KEY/master
changes; a chosen cache architecture must additionally prove bounded updates without whole-track
reanalysis or reset. Align old/new output ranges during transitions, not just visible playheads.

Acceptance records detector-independent coordinate error, audible onset/energy/peak metrics,
seams, stems, durations, native allocations, preparation time and memory. Do not declare a
production winner without listening and later 30-minute device measurements. Rollback deletes
only disposable experiment outputs; runtime and old evidence remain unchanged.
