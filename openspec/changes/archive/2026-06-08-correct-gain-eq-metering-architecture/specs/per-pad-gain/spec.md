## MODIFIED Requirements

### Requirement: Per-pad gain is applied during mixing
The audio engine SHALL apply per-pad Gain/Trim as a bounded dB channel trim while rendering each
pad contribution.

Gain/Trim MUST be applied after source or prepared-stem selection, playback-rate handling, and Key
Lock rendering, and before per-pad EQ/DSP, trigger velocity, pad summing, Master Volume, and master
output metering. Gain/Trim changes for active pads MUST remain click-safe through bounded smoothing
or ramping, and the smoothing and multiplication MUST remain realtime-safe in the audio callback.
Gain/Trim SHALL remain distinct from trigger velocity, performance volume, Master Volume, and any
future explicit output-protection feature.

The system SHALL NOT automatically change per-pad Gain/Trim in response to EQ/isolator changes,
pad clip state, or master clip state.

#### Scenario: Gain feeds EQ and later output stages
- **GIVEN** pad `id` is playing with Gain/Trim set to `g_db`
- **AND** an EQ/isolator band target is changed away from neutral
- **WHEN** the mixer renders audio
- **THEN** the pad source is trimmed by approximately `10^(g_db / 20)` before per-pad EQ/DSP
- **AND** the resulting contribution is subsequently affected by trigger velocity and Master
  Volume

#### Scenario: Gain is not hidden clip compensation
- **GIVEN** pad `id` is playing near full scale
- **WHEN** an EQ/isolator change makes the measured peak reach or exceed `1.0`
- **THEN** the system reports the peak through the relevant meters
- **AND** the pad's Gain/Trim value is not automatically reduced

