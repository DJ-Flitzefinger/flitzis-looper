# Design: gain, EQ, and metering correction

## Signal Semantics

The intended signal order for this correction is:

```text
source or prepared stem
-> playback-rate / BPM Lock / Key Lock rendering
-> per-pad Gain/Trim
-> per-pad EQ/DSP isolator
-> trigger velocity
-> selected-pad pre-master metering
-> pad summing
-> Master Volume and momentary output mute
-> master output metering
-> device output conversion
```

Per-pad Gain/Trim remains a channel input trim feeding EQ/DSP. It is not a master volume clone and
it is not an automatic compensation control. EQ/isolator changes may legitimately alter sample
peaks because filters and crossover recombination change phase relationships.

## Metering Semantics

The selected-pad meter is a local pad contribution meter. It should help the performer see the
selected pad's level after Gain/Trim and EQ/DSP, before global output controls. Master Volume
changes should not make that pad meter move.

The master output meter is the final output meter. It measures the post-sum, post-Master-Volume
signal that will be written to the output device. Unlike current pad meter projection, master peak
telemetry must preserve values above `1.0` until the Python/UI receiver decides meter rendering and
clip state. The visual meter may clamp its filled length, but the `CLIP` state must be based on the
unclamped peak reaching or exceeding `1.0`.

## Headroom And Protection Policy

The engine may use floating-point headroom internally through Gain/Trim, EQ/isolator processing,
and summing. The correction intentionally does not add hidden limiting, hidden gain compensation,
or an automatic gain drop when an isolator band is killed. Those features change sound and must be
specified as explicit optional behavior before implementation.

## DSP Topology Decision

The focused isolator peak tests prove that killing the high band can produce a modest sample-peak
rise on phase-cancelling material while unity recombination remains RMS-neutral and bounded. That
measurement does not justify changing the isolator topology, adding hidden gain compensation, or
adding automatic output protection in this change.

The implemented correction therefore keeps the current DAW-like floating-point headroom policy and
relies on accurate pad and master metering to expose peaks above full scale. Any future limiter,
master trim/headroom control, or automatic compensation remains a separate, explicit behavior
change because it would alter sound or performer control semantics.

## EQ Text Entry

Manual EQ value entry should use the same supported range as the EQ controls. It must allow an
optional leading negative sign so values such as `-6`, `-6.0`, and `-60` are reachable by typing.
Invalid characters should be rejected before entering the edit buffer, and committed values should
still be clamped to the supported range.

## Realtime Safety

Master output metering should reuse fixed-size audio-thread state and the existing bounded
audio-to-control telemetry pattern. If the telemetry channel is full, a master peak update should
be dropped without blocking. No Python objects, UI state, allocation-heavy work, file access, or
logging belongs in the callback.
