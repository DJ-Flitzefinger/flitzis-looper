## Safety gate and remaining audible work

Pinned upstream `R3LiveShifter.cpp` v4.0.0 calls `measureResamplerDelay()` from reset and from
setPitchScale while firstProcess is true. That routine allocates two vectors. The documented
steady processing/ratio-update safety therefore does not cover our cold/reset call sites.
Use the existing same-thread mutable backend ownership, moving handles rather than cloning them.

One worker services fixed per-voice recycle/ready SPSC rings. Startup constructs and silence-warms
two handles per voice before CPAL starts. One belongs to the processor and one is its reserve.
Native reset, neutral pitch setup and enough silent shifts to clear cold processing remain worker
operations. A callback invalidation clears only fixed Rust buffers and marks native state dirty.
On the next shifted render, exchange requires both a ready reserve and recycle capacity. Failure
retains the old unique handle and emits silence; dry rendering stays available. Worker failure
must not turn into an audio-thread reset or unbounded retry. Startup thread creation errors are
propagated. Worker shutdown/join and final destruction occur at engine teardown outside rendering.

The adapter starts with B-1 silent frames, where B is the immutable native block size. With r
pending input frames, the output queue retains B-1-r frames after every successful drain. This
is enough to render every next bounded segment, so changing segment size cannot add another
underflow/time slip. Native error recovery invalidates state; it does not cold-reset in realtime.
The native fixed buffers/options/resampler/precision remain unchanged.

Stem selection already crossfades the same source address before Key Lock. Wiping the pitch
processor at a mask change defeats that continuity and creates a fresh onset delay. Keep its
history and FIFOs, preserving the existing source crossfade. Stop/start/seek still invalidate;
pause/resume freezes and resumes the same processor history. Mode/neutral edges still bypass or
enter warmed shifted processing; exact audible crossfades are a subsequent source-preparation gate.

## Delay domains

Measure cold native delay after setting the initial ratio; measure warmed neutral-to-ratio output
separately. Record first nonzero onset and maximum impulse sample, since pitch processing smears
transients and first nonzero output is not a sample-accuracy claim. Add adapter lead separately;
CPAL callback-to-playback buffer estimates exclude unknown later device/hardware latency.
Do not add DSP delay to transport anchors, input timestamps or persisted markers.

Exact audible pre-roll requires a worker descriptor containing generation, unique source identity,
loop/seek policy, stem selection, exact ratio, logical source phase and future activation frame.
The worker processes bounded source lookahead and discards the exact measured delay; the callback
accepts only current on-time descriptors and retires stale state off-thread. This requires sharing
source-read policy and separating logical source telemetry from future DSP feed position. The
current safety change establishes reusable ownership and delay measurement, not that activation.

## Canonical fractional source progression

`audio_engine/source_reader.rs` shares effective half-open loop bounds, before/after-loop seek
policy, integer addressing, prepared-stem layout/version validation and same-address source
selection. It borrows accepted immutable buffers without native DSP or worker ownership.
`source_playback.rs` adds reusable scalar source progression above that policy; the mixer owns
control intent and the per-voice playback state. This path can be used by later source preparation
without importing the mixer or creating another resampling implementation.

The former interpolation scaled each render segment's input/output endpoints. Its ratio depended
on segment length, so output-anchored integer cursor equality did not imply equal samples supplied
to Rubber Band. Unanchored segment-level frame rounding also accumulated source error.
Replace both paths with an epoch containing fractional source position, actual accepted native
`f32` ratio promoted to `f64`, and active output-frame progress. Each read derives source progress
from the epoch's integer active-frame count times that ratio, rather than repeatedly adding or
rounding segment lengths. Rebase only when source/rate policy changes, preserving the fractional
remainder. Pause freezes active progress; resume retains the cursor. An in-range live loop edit
retains the source cursor, while an out-of-range edit keeps the existing loop-start clamp.
Seek/retrigger remain explicit discontinuities. Ordinary wrapping does not reset native DSP.

For each output sample, read the two integer neighbors through the shared loop/seek policy and
linearly interpolate their values. The lookahead neighbor wraps at the effective loop end; a seek
before the loop traverses the intro, and a seek after the loop traverses the tail until track end
before entering the loop. Neither interpolation tap escapes the accepted source layout.
Full-mix and prepared-stem selections share addresses and fractional source-domain transition
progress. A transition advances by consumed source distance, independent of callback count.
Integer playhead telemetry floors the next source cursor; no persisted marker, master clock or
scheduled launch target is moved.

`StretchProcessor` accepts these already-resampled planar samples directly as dry output or as
the fixed Rubber Band adapter input. It no longer performs endpoint interpolation. Its native
buffers, inverse-pitch selection, fixed-block FIFO lead and warm ownership exchange are retained.
The canonical source reader performs bounded two-tap/channel work into preallocated buffers.
No source pre-roll, future DSP feed cursor or source-prepared handle is adopted in this step.

## Output-frame tempo smoothing

The per-voice maximum ratio step remains `0.05` in dry and Key Lock modes, but its cadence is one step per `512`
active output frames instead of one step per render segment. A newly accepted target initiates
the first step immediately; subsequent steps consume fixed active-frame intervals. Render work
splits at these interval boundaries before source generation and native pitch application.
Equivalent control events therefore produce the same source ratio history under fixed,
irregular or one-frame partitions. Native pitch-update order is also equivalent when initialized
native/preparation state and prepared-reserve availability match. Asynchronous reserve starvation
still uses bounded silence; canonical source-feed equality does not depend on that availability.
Pause does not consume the interval.
Rate epochs carry their fractional source remainder into each new accepted ratio in every mode.

## Fractional feed verification and remaining gate

Use immutable nonconstant source fixtures with a separately derived continuous reference at
44.1/48/96 kHz, fractional ratios/BPM, normal wraps, intro/tail seeks, prepared-stem sums, source
selection transitions, ratio changes and pause/resume. Compare sample sequences and next
fractional cursors through fixed, irregular and one-frame partitions. The existing adapter test
only supplied identical already-varispeed samples; source tests must prove that earlier equality.
Preserve native processing and adapter-delay evidence by comparing the same canonical feed under
different partitions. Callback work remains bounded and uses accepted buffers and scalar state.

This foundation does not prove audible transient alignment. Exact-ratio source priming,
independent logical/feed cursors, explicit delay discard, stale/late prepared-state rejection and
source-aligned activation/crossfades remain pending with separate audible evidence.

## Exact-ratio source preparation proof gate

Compile `key_lock_source_preparation.rs` only in native tests. It borrows immutable full-mix/stem
buffers and uses the same `SourceReadPlan::fill_fractional_buffers` as the live mixer. It retains
separate logical and future feed copies at an explicit constant `f32` ratio. Rebase the copied
fractional source phase without changing the caller's clock or pending smoothing. Construction,
pitch setup, reset, allocation and source pre-roll remain non-realtime operations. Set the exact
inverse pitch before reset and before the first source shift, since native reset initializes its
previous output hop from that pitch. Neutral silent warming is not part of this source reference.

For native block size B and explicit experimental output discard D, process
`P = ceil((D + B - 1) / B) * B` feed frames and retain raw native output `[D..P]`.
The ready FIFO has `Q = P - D >= B - 1` frames, no pending native input, and a feed cursor at P.
During n continued output frames, feed advances by n, logical position advances only by n, and
pending input is `n mod B`. Output occupancy is `Q - (n mod B)`. Reuse the existing fixed FIFO;
do not seed the live adapter's silent lead a second time. Retain native state and both FIFOs
together. Bounded render work stages at most the fixed segment capacity and shifts only its
bounded number of complete blocks. Invalid layout/ratio/discard/render bounds fail explicitly.

An independent oracle addresses immutable source samples algebraically, then shifts them through
a separately initialized raw native instance. It must not generate its source feed with the
production source clock/reader or use the prepared fixture as its own reference. Compare every
retained and continued stereo sample with raw output `[D..D+n]` across fractional rates,
loop/intro/tail seeks, selected stems, nonaligned discard indices and one-frame/irregular segments.

D is a declared experiment, not an adopted compensation constant. Measure the uncropped native
response and translate its onset, 1%-of-peak onset and peak by D, including negative residuals.
Record discarded energy and whether the original response peak lies before D. Also measure the
retained response, which can hide a clipped startup transient if assessed alone. Cover startup
and settled markers at different block phases. Nominal API delay or bit equality alone cannot
certify an audible launch. No device is started by this gate.

Live integration remains pending: asynchronous source/generation/loop/seek/stem/exact-ratio
identity, fixed future handover frames, render splitting, stale/late rejection, off-thread
retirement, and source-aligned wet/dry/neutral transitions. The test-only fixture cannot be
adopted by `RtMixer` and changes no scheduler, transport, persisted marker or runtime fallback.

## Explicit source history and musical timing gate

The test-only request optionally declares `SourceHistory { origin, output_frames: H }`. This
origin is a prior constant-ratio source epoch, not a reverse seek inferred from the logical
position. Advance it H output frames through the shared loop/intro/tail policy; require exact
logical frame, fractional remainder and seek-mode equality before native pitch/reset/source
processing. An intro or tail may have ended before the logical cursor becomes Normal. That
endpoint cannot recover the earlier path, so the explicit origin is essential. Positive history
rejects active stem-selection transitions because their earlier ramp state was not supplied.
Zero history preserves the original proof. The declared path is hypothetical prepared content,
not evidence of which source or controls actually played previously.

H and D are independently bounded to the proof's existing 131072-frame scale. Native processing
starts at the history origin. The same P/Q/FIFO equations apply from that origin; the logical
cursor starts at H, while the feed cursor is at P. D-H is the experimental effective translation.
For raw marker time R and actual dry time T measured from the history origin, the translated
residual is `R - D - (T - H)`. D need not equal H plus API delay. Never confuse reference-suffix
equality with musical alignment or constrain experimental D to conceal failed measurements.

Keep isolated response fixtures separate from rich nonconstant loop/stem continuity fixtures.
The latter verify the shared addressing and continuation; the former measure one impulse,
8-ms source tone or damped percussion burst without subtracting native output or overlapping
loop responses. Sweep H=0/8192/16384 output frames, marker phases 0/17/511, all existing five
ratios and 44.1/48/96 kHz. Candidate translations C=D-H are API-2B, API-B, API and API+B.
All four callback partitions must equal the same independent raw suffix, including the ready
FIFO/continuation join. These three phases are a bounded diagnostic sample, not exhaustive
block-phase coverage.

Predeclare the musical engineering criterion: use pooled stereo squared energy, smoothed with a
centered 0.5-ms box window and zero extension, to report cumulative-energy q10/q50/q90 on the
original sample timeline. Centering introduces no causal-window delay. Require uncropped q10
and q50 residuals each within two ms (rounded up to an output frame), original peak retained,
at most 0.1% discarded target-response energy and at most 0.0001% energy in the capture's final
20 ms. The two-ms budget is an exploratory product gate, not a psychoacoustic universal.
Energy-envelope timing compares attack/body without correlating pitch-changing dry/wet carriers;
both q10 and q50 constrain a translation that might otherwise align only one smeared peak.
Preserve absolute/1%-of-original-peak onset, waveform peak, q90, negative residuals, cut-to-silence
jump and ready-FIFO/continuation join jump. These jumps diagnose a raw cut/join, not a click-safe
mode crossfade. Capture silence long enough to reveal response tail rather than truncating it.

Intersect timing and energy/peak retention bounds for a common C at each fixed rate/ratio/history
over every marker and burst. An empty intersection rejects even an unswept integer translation
within this criterion; it does not prove that a different onset/content policy is impossible.
Report failed criteria without changing thresholds or fitting one compensation per signal.
No selected compensation, source-aware live handover or device/deadline acceptance follows from
this non-live gate.

### Release result and prerequisite for live adoption

The bounded Windows release sweep passes 6480 exact-suffix comparisons and all 1620 groups have
identical partition metrics. No coarse candidate passes every fixture in any rate/ratio/history
group. Exact interval bounds are empty for all 36 nonneutral groups; nine ratio-1 groups alone
have feasible unswept integers. Twenty groups already fail the joint q10/q50 timing bounds before
retention is considered. All capture-tail bounds pass. At 48 kHz/ratio2/H16384, timing requires
C>=3240 and C<=2867, while energy/peak retention imposes C<=2080. API C3678 discards 96.714% of
the impulse energy, clips its original peak and yields q10/q50 residuals -907/-420 frames.

This is an engineering-criterion failure, not failed FIFO/source preparation. The independently
measured native response is intrinsically spread; adding history or translating the same response
does not remove that spread. Before source-aware live identity/adoption, resolve which musical
attack/body property must be anchored and how target onset content survives a cut or transition.
Separate additional launch-induced displacement from inherent continuous Key Lock response using
a longer-history steady reference and actual musical attack fixtures. Any new acceptance policy
needs evidence and an explicit rationale; do not merely loosen these bounds until nominal
discard passes. Preserve Rubber Band, source grid, quality/options, clocks and markers. No device
or live compensation conclusion follows from these isolated test responses.

## References

- https://raw.githubusercontent.com/breakfastquay/rubberband/v4.0.0/src/finer/R3LiveShifter.cpp
- https://breakfastquay.com/rubberband/code-doc/classRubberBand_1_1RubberBandLiveShifter.html
