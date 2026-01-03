# per-pad-eq3 Specification

## Purpose
TBD - created by archiving change add-per-pad-mixing-metering-eq. Update Purpose after archive.
## Requirements
### Requirement: Per-pad 3-band EQ parameters
The system SHALL provide a per-pad 3-band EQ for each pad sample slot id in the range `0..NUM_SAMPLES`.

The EQ SHALL expose three band controls: low, mid, and high.

Each band control SHALL be represented as a gain in decibels (dB) and SHALL default to `0.0 dB`.

Each band control SHALL support a “kill” setting such that the minimum control position maps to an effective linear gain of `0.0` for that band.

#### Scenario: Default EQ is flat
- **GIVEN** the application has started
- **WHEN** a pad has no explicitly changed EQ settings
- **THEN** the pad’s EQ gains are `0.0 dB` for low, mid, and high

#### Scenario: Low band can be killed
- **GIVEN** pad `id` is playing and has EQ gains `0.0 dB` for mid and high
- **WHEN** the performer sets the low EQ band control to its minimum (“kill”) setting
- **THEN** subsequent audio output for that pad reflects the low band being removed

### Requirement: Per-pad EQ is editable from the left sidebar
When a pad is selected, the system SHALL render three EQ controls (low/mid/high) in the left sidebar to edit the selected pad’s EQ settings.

The UI SHOULD use `imgui-knobs` (via `imgui_bundle`) for the EQ controls when available.

The UI SHALL expose a DJ-style isolator control range for each band, with a minimum “kill (−∞)” position and a maximum boost of `+6 dB`.

#### Scenario: Adjusting EQ updates playback
- **GIVEN** pad `id` is selected and is currently playing
- **WHEN** the performer adjusts one of the EQ band gains
- **THEN** subsequent audio output for that pad reflects the new EQ setting

### Requirement: EQ processing is real-time safe and high quality
The audio engine SHALL apply a real-time-safe, high-quality 3-band EQ implementation while mixing audio.

The EQ implementation MUST avoid heap allocations and blocking operations in the audio callback.

#### Scenario: EQ processing remains real-time safe
- **GIVEN** one or more pads are playing with non-default EQ settings
- **WHEN** the audio callback renders audio
- **THEN** no heap allocations occur in the audio callback due to EQ processing
- **AND** no blocking operations are performed

### Requirement: EQ composes with gain and master volume
The EQ SHALL be applied per pad in the mixing pipeline in a way that composes with per-trigger velocity, per-pad gain, and master volume.

#### Scenario: EQ and gain both affect output
- **GIVEN** pad `id` is playing
- **AND** pad gain is reduced
- **AND** an EQ band gain is increased
- **WHEN** the mixer renders audio
- **THEN** the output reflects both the gain reduction and EQ change

### Requirement: Per-pad EQ uses crossover-based isolator processing
The audio engine SHALL implement the per-pad 3-band EQ using crossover-based band splitting and summation, such that the EQ can remove an entire band with minimal bleed into neighboring bands.

The crossover filters SHALL use a 4th-order Linkwitz–Riley topology (24 dB per octave) for the low/high splits.

The crossover points SHALL be fixed (not user-adjustable) and SHALL default to `300 Hz` (low/mid) and `3.5 kHz` (mid/high).

The system SHALL split the signal into low, mid, and high components, apply the corresponding per-band gains, and sum the results back together.

#### Scenario: Unity EQ is transparent
- **GIVEN** a pad has EQ gains `0.0 dB` for low, mid, and high
- **WHEN** the mixer renders audio for that pad
- **THEN** the output is perceptually identical to the input (no unintended coloration from the EQ)

#### Scenario: Killing one band does not mute others
- **GIVEN** a pad is playing
- **WHEN** the performer sets the low band to the minimum (“kill”) setting
- **THEN** mid and high content remains audible

