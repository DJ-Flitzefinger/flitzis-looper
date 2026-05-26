## MODIFIED Requirements

### Requirement: Per-pad EQ is editable from the left sidebar
The system SHALL preserve the selected-pad left-sidebar low, mid, and high EQ controls while
routing their accepted live targets to the Rust DSP-chain isolator node.

The selected-pad EQ knobs SHALL render `0.0 dB` at the visual 12 o'clock position. The positive
half of the knob SHALL map linearly from `0.0 dB` to `+6.0 dB`; the negative half SHALL preserve
the existing logarithmic isolator kill curve stretched from `0.0 dB` down to the `-60.0 dB` kill
position.

The UI SHALL set an EQ band to `-60.0 dB` immediately when the performer right-clicks that band's
knob, and the band SHALL remain at that value until a later explicit EQ adjustment changes it.

When the performer left-drags an EQ knob, the UI SHALL use vertical mouse movement as the primary
adjustment and horizontal mouse movement as a finer adjustment at half the vertical sensitivity.

Manual EQ value entry SHALL reject typed characters before insertion unless they are digits, `.`,
or `,`; typed comma SHALL be converted to `.` before insertion.

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

#### Scenario: Manual EQ entry filters characters before insertion
- **GIVEN** the performer is manually editing an EQ value
- **WHEN** the performer types `U+00DC`
- **THEN** the character is rejected before it appears in the field
- **WHEN** the performer types `,`
- **THEN** `.` appears in the field instead
