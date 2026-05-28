# per-pad-eq3 Specification

## Purpose
Defines the performer-facing per-pad 3-band EQ as a Rust-owned DJ isolator hosted by the
internal per-pad DSP chain, while preserving compatible Python/UI project intent and realtime
safety constraints.
## Requirements
### Requirement: Per-pad 3-band EQ parameters
The system SHALL replace the hardwired per-pad EQ runtime with a per-pad 3-band DJ isolator node
hosted by the internal Rust DSP foundation for each pad sample slot id in the range
`0..NUM_SAMPLES`.

The isolator SHALL expose low, mid, and high accepted live targets as finite normalized values in
the range `0.0..1.0`. A normalized value of `0.5` SHALL be neutral, `0.0` SHALL be full kill for
that band, and `1.0` SHALL be limited boost for that band. Durable Python project intent MAY keep
the existing compatible representation, but accepted live Rust DSP targets SHALL be normalized
before sample processing.

#### Scenario: Default EQ is flat
- **GIVEN** the application has started
- **WHEN** a pad has no explicitly changed EQ settings
- **THEN** the accepted live low, mid, and high isolator targets are neutral
- **AND** the rendered per-pad EQ path is transparent within floating-point tolerance

#### Scenario: Low band can be killed
- **GIVEN** pad `id` is playing with mid and high isolator targets at neutral
- **WHEN** the performer sets the low EQ band control to its minimum kill setting
- **THEN** Rust receives an accepted normalized low-band target of `0.0`
- **AND** subsequent audio output for that pad reflects the low band being removed
- **AND** mid and high content remains audible

### Requirement: Per-pad EQ is editable from the left sidebar
The system SHALL preserve the selected-pad left-sidebar low, mid, and high EQ controls while
routing their accepted live targets to the Rust DSP-chain isolator node.

The selected-pad EQ knobs SHALL render `0.0 dB` at the visual 12 o'clock position. The positive
half of the knob SHALL map linearly from `0.0 dB` to `+6.0 dB`; the negative half SHALL preserve
the logarithmic isolator kill curve stretched from `0.0 dB` down to the `-60.0 dB` kill position.

The UI SHALL set an EQ band to `-60.0 dB` immediately when the performer right-clicks that band's
knob, and the band SHALL remain at that value until a later explicit EQ adjustment changes it.
When the performer left-drags an EQ knob, the UI SHALL use vertical mouse movement as the primary
adjustment and horizontal mouse movement as a finer adjustment at half the vertical sensitivity.

Manual EQ value entry SHALL accept finite typed values in the supported performer-facing EQ range,
including valid negative values such as `-6`, `-6.0`, and `-60`. The input filter SHALL allow an
optional leading `-`, digits, `.`, and `,`; typed comma SHALL be converted to `.` before insertion.
The input filter SHALL reject invalid characters before they appear in the edit field. Committed
manual values SHALL be clamped to the supported `-60.0..=+6.0 dB` range before they are stored or
published to Rust.

The UI SHALL preserve the existing neutral reset gesture for each band. Keyboard and MIDI mappings
for per-pad EQ SHALL continue to use stable action semantics outside the audio callback, derive
bounded controller-owned target changes, and publish accepted targets through the bounded
parameter path.

#### Scenario: Neutral EQ is centered visually
- **GIVEN** pad `id` is selected
- **AND** its low EQ band is `0.0 dB`
- **WHEN** the left-sidebar EQ controls are rendered
- **THEN** the low EQ knob points to 12 o'clock

#### Scenario: Right-click kills one EQ band
- **GIVEN** pad `id` is selected
- **WHEN** the performer right-clicks the mid EQ knob
- **THEN** the selected pad's mid EQ value becomes `-60.0 dB`
- **AND** no right-drag gesture is required to keep that value

#### Scenario: Horizontal left-drag is finer than vertical drag
- **GIVEN** pad `id` is selected
- **WHEN** the performer left-drags a high EQ knob horizontally by the same pixel distance as a
  vertical drag
- **THEN** the horizontal drag produces half the knob-position change of the vertical drag

#### Scenario: Manual EQ entry accepts negative values
- **GIVEN** the performer is manually editing the selected pad's high EQ value
- **WHEN** the performer types `-6.0` and commits the field
- **THEN** the selected pad's high EQ value becomes `-6.0 dB`
- **AND** the accepted live target is published through the existing bounded parameter path

#### Scenario: Manual EQ entry accepts full kill
- **GIVEN** the performer is manually editing an EQ value
- **WHEN** the performer types `-60` and commits the field
- **THEN** the committed value is accepted as the band kill value

#### Scenario: Manual EQ entry rejects invalid characters before insertion
- **GIVEN** the performer is manually editing an EQ value
- **WHEN** the performer types `U+00DC`
- **THEN** the character is rejected before it appears in the field
- **WHEN** the performer types `,`
- **THEN** `.` appears in the field instead

#### Scenario: Adjusting EQ updates playback
- **GIVEN** pad `id` is selected and currently playing
- **WHEN** the performer adjusts one EQ band
- **THEN** the controller derives a bounded accepted target for that band
- **AND** Rust applies the target to the per-pad isolator node through smoothed DSP state
- **AND** subsequent audio output for that pad reflects the new EQ setting

#### Scenario: Neutral reset remains available
- **GIVEN** pad `id` has a non-neutral EQ band setting
- **WHEN** the performer uses the existing neutral reset gesture for that band
- **THEN** the accepted live target for that band returns to normalized `0.5`
- **AND** the durable project intent remains compatible with project restore

### Requirement: EQ processing is real-time safe and high quality
The audio engine MUST apply the per-pad 3-band DJ isolator as realtime-safe Rust DSP-chain
processing while mixing audio.

Isolator processing in the CPAL callback MUST avoid heap allocations, disk I/O, JSON access,
Python/GIL access, UI calls, blocking operations, logging, neural inference, plugin
loading/scanning, unbounded loops, and long-running work. Required node state, coefficient state,
and scratch state SHALL be prepared outside realtime rendering or stored as fixed-size Rust-owned
audio state.

#### Scenario: EQ processing remains real-time safe
- **GIVEN** one or more pads are playing with non-neutral isolator targets
- **WHEN** the audio callback renders audio
- **THEN** isolator processing touches only already owned audio buffers and fixed-size Rust state
- **AND** no heap allocations, blocking operations, disk access, logging, plugin scanning, or
  Python/GIL interaction occur due to EQ processing

### Requirement: EQ composes with gain and master volume
The system SHALL apply EQ per pad through the Rust DSP-chain path after source selection, loop
wrapping, playback-rate handling, Key Lock rendering, and per-pad Gain/Trim, and before
per-trigger velocity, selected-pad pre-master metering, pad summing, Master Volume, master output
metering, and telemetry.

The replacement SHALL NOT apply both the old hardwired EQ path and the new isolator node to the
same rendered voice.

#### Scenario: EQ and gain both affect output
- **GIVEN** pad `id` is playing
- **AND** pad gain is reduced
- **AND** an isolator band target is changed away from neutral
- **WHEN** the mixer renders audio
- **THEN** the source is first trimmed by per-pad Gain/Trim
- **AND** the trimmed signal is then processed by the isolator node
- **AND** the pad is not processed by a second hardwired EQ stage

### Requirement: Per-pad EQ uses crossover-based isolator processing
The audio engine SHALL implement the per-pad 3-band EQ replacement using crossover-based DJ
isolator band splitting and summation owned by the internal Rust DSP node.

The low band SHALL cover content below `250 Hz`. The mid band SHALL cover content from `250 Hz`
to `4 kHz`. The high band SHALL cover content above `4 kHz`. The initial crossover design SHALL
target 4th-order Linkwitz-Riley-style recombination. At neutral targets, the summed output SHALL
match the input within floating-point tolerance. At minimum target, the selected band SHALL be
fully killed. At maximum target, boost SHALL be limited to `+6 dB`.

Accepted target changes SHALL be smoothed on the Rust audio side before sample processing.

#### Scenario: Unity EQ is transparent
- **GIVEN** a pad has neutral low, mid, and high isolator targets
- **WHEN** the mixer renders audio for that pad
- **THEN** the per-pad isolator output matches the input within floating-point tolerance

#### Scenario: Killing one band does not mute others
- **GIVEN** a pad is playing
- **WHEN** the performer sets the low band to the minimum kill setting
- **THEN** low-frequency content is removed from that pad
- **AND** mid and high content remains audible

#### Scenario: Representative low and high band-center kills are suppressed
- **GIVEN** deterministic representative tones are rendered through the per-pad isolator at
  `48 kHz`
- **WHEN** the low band is set to minimum kill while mid and high remain neutral
- **THEN** `60 Hz` low-band-center content is strongly suppressed
- **AND** `8 kHz` high-band content remains audible
- **WHEN** the high band is set to minimum kill while low and mid remain neutral
- **THEN** `8 kHz` high-band-center content is strongly suppressed
- **AND** `60 Hz` low-band content remains audible

#### Scenario: Rapid EQ target changes are smoothed
- **GIVEN** pad `id` is playing
- **WHEN** repeated accepted targets for one isolator band reach Rust through the parameter path
- **THEN** the audio side coalesces and smooths the accepted target before sample processing
- **AND** the callback stays bounded and nonblocking

### Requirement: Isolator Replacement Removes Hardwired Mixer EQ Authority
The system SHALL remove the current standalone hardwired mixer EQ path as the live EQ authority
when the per-pad DJ isolator replacement is implemented.

Compatibility APIs may continue to accept existing per-pad EQ calls, project restore values, and
mapping actions, but the live audio state SHALL be owned by typed DSP parameter identities and the
per-pad isolator node. The replacement SHALL NOT keep separate mixer EQ coefficients or per-voice
hardwired EQ state active as a second processing stage.

#### Scenario: Compatibility setter targets the DSP node
- **GIVEN** existing controller code calls the per-pad EQ setter for pad `id`
- **WHEN** the replacement is active
- **THEN** the accepted live low, mid, and high targets are routed to typed per-pad DSP parameter
  identities
- **AND** the rendered voice is processed by the isolator node rather than by standalone mixer EQ
  coefficients

#### Scenario: Project restore does not double-apply EQ
- **GIVEN** a project restores non-neutral EQ settings for pad `id`
- **WHEN** playback starts after restore
- **THEN** the restored settings initialize the per-pad isolator node targets
- **AND** the old hardwired EQ path is not also applied to the same audio
