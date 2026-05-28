# master-output-metering Specification

## Purpose
Defines final master output peak telemetry, runtime projection, UI rendering, clip indication, and
the explicit headroom policy for output protection.

## Requirements

### Requirement: Master output peak telemetry
The audio engine SHALL compute master output peak levels after all active pad contributions are
summed and after Master Volume, momentary output mute, and other current global output gain controls
are applied.

Master output peaks MUST be measured before final device-format conversion or any output-device
clamping. Peak values MUST be mono, computed as the maximum absolute sample value across output
channels for the measurement interval. Master output peak telemetry MUST preserve finite values
above `1.0` when the rendered floating-point output exceeds full scale so the control/UI layer can
derive accurate clip state.

The audio engine SHALL publish master output peak updates from the audio thread to the control/UI
thread through bounded, nonblocking telemetry.

#### Scenario: Master peak is post-sum and post-master-volume
- **GIVEN** two pads are playing and Master Volume is `0.5`
- **WHEN** the audio callback renders and sums their output
- **THEN** the master peak telemetry is derived from the summed output after the `0.5` Master
  Volume multiplier is applied

#### Scenario: Master peak above full scale is preserved
- **GIVEN** the rendered floating-point master output reaches a peak of `1.25`
- **WHEN** the audio engine publishes a master output peak update
- **THEN** the telemetry value remains approximately `1.25`
- **AND** it is not clamped to `1.0` before the control/UI layer receives it

#### Scenario: Master peak telemetry is nonblocking
- **GIVEN** the audio-to-control message channel is full
- **WHEN** the audio thread attempts to publish a master output peak update
- **THEN** the update is dropped
- **AND** the audio thread continues without blocking or panicking

### Requirement: Master output meter and clip hold
The system SHALL project master output peak telemetry into runtime UI state and render a master
output meter in or directly integrated with the Master Volume control area.

The master output meter SHALL represent the final post-sum, post-Master-Volume output level rather
than the selected pad level. The visual fill MAY clamp to the displayable meter range, but clip
state MUST be derived from the unclamped master output peak. When the unclamped master output peak
reaches or exceeds `1.0`, the UI SHALL show a clear master `CLIP` indication that remains visibly
held for about one second unless refreshed by another clipping peak.

#### Scenario: Master meter reflects final output
- **GIVEN** one pad is playing with a steady selected-pad pre-master peak
- **WHEN** the performer lowers Master Volume
- **THEN** the master output meter decreases
- **AND** the selected-pad pre-master meter does not decrease solely because Master Volume changed

#### Scenario: Master clip indication holds briefly
- **GIVEN** the master output telemetry reports an unclamped peak of `1.1`
- **WHEN** the Master Volume control area is rendered immediately after the update
- **THEN** the master `CLIP` indication is visibly active
- **WHEN** about one second elapses without another clipping master peak
- **THEN** the master `CLIP` indication is no longer active

### Requirement: Floating-point headroom and explicit output protection
The system SHALL allow Gain/Trim, EQ/isolator processing, and summing to use floating-point
headroom internally without silently applying hidden automatic limiting, hidden gain compensation,
or an automatic output trim.

If output protection such as limiting, automatic gain compensation, or a master trim/headroom
control is added later, it MUST be specified as an explicit user-visible behavior with its own
controls, defaults, tests, and realtime-safety constraints. The absence of hidden protection SHALL
not prevent the system from accurately metering peaks above full scale.

#### Scenario: Isolator peak rise is metered rather than hidden
- **GIVEN** a pad is playing near full scale
- **WHEN** an EQ/isolator band is killed and the resulting waveform peak rises above `1.0`
- **THEN** the system reports the peak through metering and clip state
- **AND** it does not silently reduce Gain/Trim, alter the EQ target, or apply an automatic limiter
