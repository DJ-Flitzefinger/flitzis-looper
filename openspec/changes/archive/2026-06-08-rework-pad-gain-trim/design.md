# Design: dB pad Gain/Trim

## Signal Placement

The implemented signal order is:

```text
Pad/source playback
-> stem source selection or full mix
-> playback-rate / BPM Lock / Key Lock rendering
-> pad Gain / Trim
-> per-pad EQ/DSP chain
-> trigger velocity
-> master volume
-> master mix/output
```

This differs from the previous architecture note where EQ preceded per-pad gain. For a
professional Trim control, Gain is the source/channel input level feeding the EQ and later mix
stages, while volume/master volume remain performance and output-level controls. The Rust mixer is
the correct place because it already owns source selection, per-pad DSP, parameter application,
and metering.

## Parameter Representation

Python stores durable intent as `pad_gain_db`, a per-pad list of dB values. The old `pad_gain`
project key is treated as legacy input only:

- values `0.0..1.0` are interpreted as old linear gain;
- values `1.0..100.0` are interpreted as old percent gain;
- old unity `1.0` or `100` migrates to `0.0 dB`;
- old values below unity map through `20 * log10(linear)` and clamp to `-12.0 dB`;
- missing values default to `0.0 dB`.

The Python/Rust API method name remains `set_pad_gain` for compatibility, but its accepted value
is now Gain/Trim in dB.

## Realtime Safety

Rust receives bounded dB values through the existing fast parameter ring. The callback coalesces
targets by pad id, converts the latest dB target to linear gain, and updates a fixed-size per-pad
smoother. Active changes ramp over a short fixed time; inactive pads snap to the target so restored
projects start at the intended trim value. The render loop applies only a smoothed scalar
multiplication per sample frame and does not allocate, block, call Python, read files, log, or
perform unbounded work.

## Metering

The existing Rust per-pad peak telemetry remains the source of meter truth. The UI renders that
cached peak near the selected-pad Gain control using a horizontal scale with the first 80% green
and the final 20% yellow. Clipping is intentionally shown by the separate `CLIP` indicator rather
than by a tiny red range inside the scale. The previous vertical right-edge meter inside each pad
button is removed to avoid duplicate level displays; loaded pads still keep their pad number,
track label, status styling, stem indicator, and BPM/key metadata overlay. The clip hold timer
lives in `SessionState` and holds for about one second. The meter is not derived from knob
position.
