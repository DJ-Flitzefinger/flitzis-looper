## MODIFIED Requirements

### Requirement: Global Speed Controls
The system SHALL expose the right-side global Speed/Pitch control as a displayed-BPM control
whenever an effective BPM reference is available.

When a BPM reference is available, left-click plus and minus actions SHALL adjust the displayed BPM
by exactly 0.1 BPM per activation, right-click plus and minus actions SHALL adjust the displayed BPM
by exactly 1.0 BPM per activation, and the mouse/slider control SHALL snap movement to a 0.1 BPM
grid. Hovering the Pitch control and scrolling the mouse wheel SHALL adjust the displayed BPM by
exactly 1.0 BPM per wheel movement. When a plus or minus action is held with the left mouse button,
the system SHALL continue
applying 0.1 BPM adjustments at a bounded repeat rate after an initial hold delay. When a plus or
minus action is held with the right mouse button, the system SHALL continue applying 1.0 BPM
adjustments at the same bounded repeat cadence after the initial hold delay. The control SHALL
convert the selected displayed BPM back to the existing bounded speed multiplier before sending
updates to the audio engine. The speed multiplier range remains 0.5x..2.0x and the default remains
1.00x. The less prominent value rendered inside the Pitch fader SHALL continue to show the speed
multiplier factor, not the BPM value, even though the interaction grid is BPM-based. If no effective
BPM reference is available, the system MAY preserve the existing bounded speed-multiplier fallback
because no BPM-unit conversion can be derived.

This UI/controller behavior SHALL NOT add disk I/O, Python/GIL access, logging, blocking work,
heavy allocation, neural inference, or any new work to the Rust audio callback.

#### Scenario: Plus and minus adjust displayed BPM by one tenth
- **GIVEN** the selected pad has an effective BPM of 120.0
- **AND** the global speed multiplier is 1.00x
- **WHEN** the performer activates the Pitch plus action once
- **THEN** the displayed BPM becomes approximately 120.1
- **AND** the speed multiplier sent to the audio engine is approximately `120.1 / 120.0`
- **WHEN** the performer activates the Pitch minus action once
- **THEN** the displayed BPM becomes approximately 120.0

#### Scenario: Right-click plus and minus adjust displayed BPM by one
- **GIVEN** the selected pad has an effective BPM of 120.0
- **AND** the global speed multiplier is 1.00x
- **WHEN** the performer right-clicks the Pitch plus action once
- **THEN** the displayed BPM becomes approximately 121.0
- **AND** the speed multiplier sent to the audio engine is approximately `121.0 / 120.0`
- **WHEN** the performer right-clicks the Pitch minus action once
- **THEN** the displayed BPM becomes approximately 120.0

#### Scenario: Holding plus repeats BPM nudges
- **GIVEN** the selected pad has an effective BPM of 120.0
- **AND** the global speed multiplier is 1.00x
- **WHEN** the performer holds the Pitch plus action past the repeat delay
- **THEN** the system continues increasing the displayed BPM in bounded 0.1 BPM ticks until the
  action is released or the speed limit is reached

#### Scenario: Holding right-click plus repeats one-BPM nudges
- **GIVEN** the selected pad has an effective BPM of 120.0
- **AND** the global speed multiplier is 1.00x
- **WHEN** the performer holds the Pitch plus action with the right mouse button past the repeat delay
- **THEN** the system continues increasing the displayed BPM in bounded 1.0 BPM ticks until the
  action is released or the speed limit is reached

#### Scenario: Mouse movement snaps to BPM tenths
- **GIVEN** the selected pad has an effective BPM of 120.0
- **WHEN** the performer drags the Pitch control near 123.14 BPM
- **THEN** the control snaps the target to approximately 123.1 BPM
- **AND** the speed multiplier sent to the audio engine is approximately `123.1 / 120.0`

#### Scenario: Pitch fader still displays speed factor
- **GIVEN** the global speed multiplier is 1.50x
- **WHEN** the Pitch fader is rendered
- **THEN** the value inside the fader shows approximately `1.50`
- **AND** the top BPM display continues to show the current displayed BPM

#### Scenario: Wheel and middle click adjust Pitch while hovered
- **GIVEN** the selected pad has an effective BPM of 120.0
- **WHEN** the performer hovers the Pitch control and scrolls the mouse wheel upward once
- **THEN** the displayed BPM increases by approximately 1.0 BPM
- **WHEN** the performer clicks the mouse wheel while hovering the Pitch control
- **THEN** the speed multiplier resets to 1.00x

#### Scenario: Speed multiplier fallback remains bounded without BPM
- **GIVEN** no effective BPM reference is available
- **WHEN** the performer changes the Pitch control
- **THEN** the system keeps speed changes within the existing 0.5x..2.0x bounds

### Requirement: Wheel And Middle-Click Gestures For Continuous Controls
The system SHALL allow hovered continuous controls to respond to mouse-wheel nudges and mouse-wheel
button reset clicks.

Hovering Master Volume then scrolling the mouse wheel SHALL adjust the control by five percentage
points per wheel movement. Hovering Gain then scrolling the mouse wheel SHALL adjust the control by
one percentage point per wheel movement. Hovering a per-pad EQ control then scrolling the mouse
wheel SHALL adjust that EQ band by 1.0 dB per wheel movement. Hovering Master Volume and clicking
the mouse wheel SHALL reset it to 100 percent. Hovering Gain and clicking the mouse wheel SHALL
reset it to 100 percent. Hovering a per-pad EQ control and clicking the mouse wheel SHALL reset
that band to 0.0 dB.

These gestures SHALL remain UI/controller behavior and SHALL NOT add disk I/O, Python/GIL access,
logging, blocking work, heavy allocation, neural inference, or any new work to the Rust audio
callback.

#### Scenario: Master Volume wheel and middle-click gestures
- **GIVEN** Master Volume is 50 percent
- **WHEN** the performer hovers Master Volume and scrolls upward once
- **THEN** Master Volume becomes approximately 55 percent
- **WHEN** the performer clicks the mouse wheel while hovering Master Volume
- **THEN** Master Volume resets to 100 percent

#### Scenario: Gain wheel and middle-click gestures
- **GIVEN** the selected pad Gain is 50 percent
- **WHEN** the performer hovers Gain and scrolls downward once
- **THEN** Gain becomes approximately 49 percent
- **WHEN** the performer clicks the mouse wheel while hovering Gain
- **THEN** Gain resets to 100 percent

#### Scenario: EQ wheel and middle-click gestures
- **GIVEN** the selected pad Mid EQ is 0.0 dB
- **WHEN** the performer hovers Mid EQ and scrolls upward once
- **THEN** Mid EQ becomes approximately 1.0 dB
- **WHEN** the performer clicks the mouse wheel while hovering Mid EQ
- **THEN** Mid EQ resets to 0.0 dB

### Requirement: Pitch Control Center Indicator
The system SHALL render a small horizontal center-position indicator beside the Pitch control at
the 1.00x/default speed position.

The indicator SHALL align with the Pitch fader's neutral grab position using the same usable slider
track geometry as the rendered fader. The indicator SHALL be green only while the speed multiplier
is at the neutral 1.00x/default speed position; otherwise it SHALL use the same grey as the Pitch
fader grab. The indicator SHALL be visual only and SHALL NOT perform file access, analysis, audio
work, or input mapping dispatch.

#### Scenario: Center indicator marks neutral pitch
- **GIVEN** the performance view is rendered
- **WHEN** the Pitch control is visible
- **THEN** a small horizontal marker is drawn beside the control at the neutral 1.00x position

#### Scenario: Center indicator color follows neutral state
- **GIVEN** the speed multiplier is 1.00x
- **WHEN** the Pitch control is rendered
- **THEN** the center indicator is green
- **WHEN** the performer changes Pitch away from 1.00x
- **THEN** the center indicator is grey

### Requirement: BPM Display Manual Entry
The system SHALL allow the performer to double-click the right-side BPM display and type a target
BPM with at most two decimal places.

The BPM entry SHALL accept only digits, `.`, and `,` at input time, so disallowed characters SHALL
not appear in the field. The entry SHALL interpret `,` as `.` and SHALL ignore all other typed
characters. The committed BPM SHALL be converted to the existing bounded speed multiplier using
the current BPM reference, and invalid or non-positive entries SHALL NOT update the speed.

This manual entry SHALL remain Python/UI control-plane behavior and SHALL NOT add disk I/O,
Python/GIL access, logging, blocking work, heavy allocation, neural inference, or any new work to
the Rust audio callback.

#### Scenario: Double-click opens exact BPM entry
- **GIVEN** the BPM display shows 120.00
- **WHEN** the performer double-clicks the BPM display
- **THEN** the display becomes a text entry initialized to `120.00`

#### Scenario: Comma input is normalized and limited to two decimals
- **GIVEN** the BPM entry is active
- **WHEN** the performer types `123,456abc`
- **THEN** the entry buffer becomes `123.45`
- **AND** committing the entry targets 123.45 BPM

#### Scenario: Invalid BPM entry is ignored
- **GIVEN** the BPM entry is active
- **WHEN** the performer commits an empty, zero, or non-positive value
- **THEN** the speed multiplier is not updated
