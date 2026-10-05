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

## References

- https://raw.githubusercontent.com/breakfastquay/rubberband/v4.0.0/src/finer/R3LiveShifter.cpp
- https://breakfastquay.com/rubberband/code-doc/classRubberBand_1_1RubberBandLiveShifter.html
