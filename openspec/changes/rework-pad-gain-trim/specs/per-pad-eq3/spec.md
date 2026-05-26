## MODIFIED Requirements

### Requirement: EQ composes with gain and master volume
The EQ SHALL be applied per pad through the Rust DSP-chain path after source selection, loop
wrapping, playback-rate and Key Lock processing, and after the pad Gain/Trim stage, but before
per-trigger velocity, master volume, metering, and telemetry.

The replacement SHALL NOT apply both the old hardwired EQ path and the new isolator node to the
same rendered voice. The Gain/Trim stage SHALL feed the EQ/DSP chain so the trim behaves as a
professional source/channel input level rather than as a performance or master volume clone.

#### Scenario: EQ and gain both affect output
- **GIVEN** pad `id` is playing
- **AND** pad Gain/Trim is reduced
- **AND** an isolator band target is changed away from neutral
- **WHEN** the mixer renders audio
- **THEN** the pad source is first scaled by Gain/Trim
- **AND** the output reflects both the Gain/Trim change and the isolator change
- **AND** the pad is not processed by a second hardwired EQ stage
