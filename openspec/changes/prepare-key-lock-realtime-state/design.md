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

## References

- https://raw.githubusercontent.com/breakfastquay/rubberband/v4.0.0/src/finer/R3LiveShifter.cpp
- https://breakfastquay.com/rubberband/code-doc/classRubberBand_1_1RubberBandLiveShifter.html
