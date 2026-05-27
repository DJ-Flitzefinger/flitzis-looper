# Change: Correct gain, EQ, and metering architecture

## Why

The current UI can show a selected-pad meter near full scale while the final master output level is
not visible. When the DJ isolator kills a band, sample peaks can rise because crossover phase and
recombination can reduce cancellation even though less frequency content remains audible. A
pre-master pad meter cannot prove whether the final summed output clipped, and clamped telemetry
cannot show how far above full scale the master output went.

The selected-pad EQ text field also needs to accept valid negative values such as `-6`, `-6.0`,
and `-60` so typed EQ edits match the intended isolator kill range.

## What Changes

- Add explicit master output peak metering after pad summing and after Master Volume.
- Preserve unclamped master peak telemetry so the UI can distinguish below-full-scale peaks from
  values that exceeded `1.0`.
- Render master output level in the Master Volume control area with a clear `CLIP` hold of about
  one second.
- Clarify that the selected-pad meter is a pre-master per-pad meter derived from the pad's rendered
  contribution, not from Master Volume or final output level.
- Clarify that internal Gain/Trim, EQ/isolator, and summing may use floating-point headroom and
  must not silently apply hidden limiting or automatic gain compensation.
- Allow optional leading negative signs in manual EQ value entry while continuing to reject invalid
  characters and clamp to the supported EQ range.

## Non-Goals

- No change to the DJ isolator topology in this change.
- No automatic limiter, hidden gain compensation, or output protection feature.
- No new plugin hosting, deck/group/master FX chain, sample loading behavior, stem behavior, or
  persistence format beyond any state needed for master metering projection.
- No change to the realtime callback safety boundary.

## Realtime Constraints

The master meter and related telemetry MUST remain realtime safe. The CPAL audio callback MUST NOT
perform disk I/O, JSON access, Python/GIL work, UI calls, blocking operations, logging, plugin
loading/scanning, neural inference, unbounded loops, heavy allocation, or long-running work.

