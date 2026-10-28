> Historical renderer evidence for the removed source-SYNC feature. See [the archive](sync-archive.md).

# Accepted-map renderer comparison

The first comparison using the actual shared SourceBeatmap inverse does **not**
qualify either candidate for live mapped playback. The shared inverse, mapped-reader
coordinates and declared partition protocol pass; acoustic and pitch predicates retain substantial failures
and missing evidence. Production playback remains scalar.

## Frozen inputs and execution

The isolated Rust diagnostic uses complete generated stereo PCM at 48 kHz. Five
separately hashed fixtures contain stereo 160/211 Hz tones or one 384-source-frame
attack at one of four declared positions. Their accepted maps share geometry:

| Quarter coordinate |0|2|4|6|8|
| --- | --- | --- | --- | --- | --- |
| Source frame |0|16384|49152|114688|163840|
| Master output frame |0|32768|65536|98304|131072|

The resulting physical rates are 0.5, 1, 2, 1.5. Each fixture checks all 131,073 inverse
positions against an independent integer/half-integer oracle. Complete PCM and mono
hashes bind the descriptor; this is synthetic consistency, not current live-source
custody. Trusted unknown-meter quarter geometry works; a gap refuses before any
candidate Native owner is constructed.

The comparison covers k=0,+7,-23,+24, both Key Lock states, phase offsets 0/17 output frames,
and 512/irregular caller partitions. Fractional source reads use SourcePlayback and
SourceReadPlan. LiveShifter uses the existing NativeAdapterState/FIFO. OfflineR3
uses the existing study/process/retrieve owner with four positive map keyframes;
implicit 0->0 and the true exclusive end are preserved. Old affine assertions remain.
These keyframes express the same requested anchor correspondence; Stretcher's
internal per-sample warp is not instrumented or certified as the canonical PWA inverse.

Offline Key Lock OFF requires changing pitch h*r, which its fixed-pitch offline API
cannot automate. That combination is explicitly unsupported. Offline phase17 also
remains unsupported: no rotated/cropped render is presented as equivalent.
The pinned [Rubber Band offline API](https://github.com/breakfastquay/rubberband/blob/v4.0.0/rubberband/RubberBandStretcher.h)
describes the fixed-pitch and preplanned-keyframe distinction.

## Measured result, 2026-10-21

The Release diagnostic completed in 15.98 s after a 2 min 19 s test build. It retained
165 Native captures, including five parent/component runs, and 40 Offline R3 captures.
All 80 Native and 20 Offline complete-output partition pairs are bit-identical under
the explicitly canonicalized Native block protocol. This does not certify arbitrary
production callback or intrablock pitch handling.

The following tables count distinct configurations once, omitting the identical
irregular-caller repetitions. They are measurement predicates, not counts of tests
or whole-B5 acceptance.

| Original finite-capture acoustic predicates | All within bounds | At least one failure |
| --- | ---: | ---: |
| Mapped reader + cold Native/FIFO |0|64|
| OfflineR3, Key Lock ON, phase0|2|14|

The fixed predicates retain 2 ms q10/q50 timing, 0.001 pre-marker energy, 1e-6 final
20 ms energy and peak retention. q90 remains separately reported. Native rows use
the raw natural output clock, without fitted translation, gain or crop. Their cold
startup delay is not evidence that the productive prepared-history route fails.
Nominal delay is recorded separately and is not silently used as a fitted remedy.

| Tonal windows | Eligible and below15cents | Eligible, outside bound | Insufficient evidence |
| --- | ---: | ---: | ---: |
| Mapped reader + Native/FIFO |105|7|16|
| OfflineR3 |19|9|4|

Original tonal guards remain: finite samples, mean energy >0.001, more than 8 positive
crossings and absolute error <15 cents. Low-pitch windows with too few cycles remain
unqualified even if a numerical frequency can be printed. Expected frequency uses
an independent raw-semitone oracle. The earlier provisional run omitted evidence
guards; its scores are superseded, with the negative history retained locally.

Four synthetic gain components share the parent map but use independent serial
Native instances. Their raw sum differs from the parent by at most 1.49e-8; the
processed sum differs by about 0.338 maximum and 0.106 RMS. This separates raw rounding
from processed audio divergence. It is not a PreparedStemSet/mask/lifetime test or
separator-quality measurement.

Actual linked RubberBand DLL: 298,496 bytes, SHA256
`4a8d323cc8c40d16413bbffe459811f93ebb5f7249b9e830bf4d11c62e4feb6d`.
The final test executable was 80,945,152 bytes, SHA256
`6238c45adfc9fc7a45410fea56e9edd40a050e169330290740fe062a2f5612b6`.
Loaded-function/IAT binding was checked; the R3 engine query is not a library
semantic-version claim. Complete identities and positive/negative rows remain in
the existing local test log and owned diagnostic logs.

## Limits and next integration

Native output retains 131,072 mapped frames plus 32,768 zero-fed frames. This is the
entire declared finite capture, not proof of internal Native-tail exhaustion.
Phase 17 exposes four mixed-pitch FIFO blocks under Key Lock; the current adapter
assigns the completing subchunk's pitch to the pending Native block. Its distant
settled windows do not establish quality at that transition. Raw loop-seam steps
are observations at the canonical input seam, not latency-corrected audible-seam
acceptance. Synthetic components do not cover actual stem generations or masks.

The next integration must preserve the accepted map revision and exact fractional
trajectory through original prepared-source/history/output-range ownership, with
truthful unsupported boundaries and current source/loop/tempo/pitch identities.
Simply feeding local rates into the scalar glide is not the canonical inverse.
No production winner or live activation follows from this comparison. All 37 offsets,
total -23..+24, other rates/layouts/maps, actual prepared stems, complete transition/
resource/RT, musical and genuine Human/device gates remain open.

The separate real Signalsmith Stretch default-preset canonical q1/p2 prerequisite
also fails all four kick cases: its unity coordinate control passes, but original
attack energy and peak retention fail. It remains an isolated nondefault test
feature. [Measured results and fixed criteria](../openspec/changes/probe-signalsmith-canonical-pitch/results.md)
retain the negative result; no backend or admission change follows.

Run the opt-in diagnostic through the normal Windows Native test wrapper:

```powershell
$env:UV_NO_SYNC = '1'
./scripts/run-rust-tests.ps1 -CargoArgs @('--release', 'compare_accepted_nonuniform_map_renderers', '--', '--ignored', '--nocapture', '--test-threads=1')
```

Affected regressions also cover the two legacy Stretcher probes, 17 source-clock
tests and two shared-map fixture tests. No production API/ABI or installer changed;
the installed Release remains unchanged. No App, model, device or stream was started.
