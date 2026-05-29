## ADDED Requirements

### Requirement: Master Volume Direct Mute Gesture
The system SHALL set Master Volume to the persisted minimum value when the performer right-clicks
the Master Volume slider.

The direct mute gesture SHALL move the visible Master Volume slider to the leftmost position and
SHALL persist the same `0.0` value as other explicit Master Volume edits. This UI/controller
behavior SHALL NOT add disk I/O, Python/GIL access, logging, blocking work, heavy allocation,
neural inference, or any new work to the Rust audio callback.

#### Scenario: Right-clicking Master Volume sets it to zero
- **GIVEN** Master Volume is above zero
- **WHEN** the performer right-clicks the Master Volume slider
- **THEN** Master Volume becomes `0.0`
- **AND** the slider is rendered at its leftmost position

### Requirement: Bottom-Bar START/STOP Control
The system SHALL render a START/STOP button immediately to the left of the bottom-right Settings
toggle.

The START/STOP button SHALL be vertically centered on the same horizontal line as the bottom-bar
icons and SHALL be wider than an icon button so it can be used as a performance control. While no
remembered global stop is active, the START/STOP button SHALL render in the active green style.
After the performer right-presses START/STOP to stop playback, the START/STOP button SHALL render in
the red off style until the remembered loop set is restarted or cleared.

#### Scenario: START/STOP button is aligned beside Settings
- **WHEN** the bottom bar is rendered
- **THEN** the START/STOP button appears directly left of the Settings toggle
- **AND** both controls are vertically centered within the bottom bar

#### Scenario: START/STOP button reflects stopped state
- **GIVEN** no global START/STOP stop state is active
- **WHEN** the bottom bar is rendered
- **THEN** the START/STOP button is green
- **WHEN** the performer presses the right mouse button on START/STOP
- **THEN** the START/STOP button becomes red

### Requirement: START/STOP Left Mouse Starts Or Restarts
The system SHALL make left mouse button down on the bottom-bar START/STOP button start or restart
the current target loop set immediately, using the same mouse-down timing style as pad triggering.

If a remembered global stop is active, START/STOP left mouse down SHALL start the remembered pads
together from the beginning of each effective loop. If no remembered global stop is active,
START/STOP left mouse down SHALL restart all currently playing pads from the beginning of their
effective loops. This action SHALL NOT stop playback.

#### Scenario: Left mouse down restarts current active loops
- **GIVEN** pads 1 and 2 are currently active
- **WHEN** the performer holds the left mouse button down on START/STOP
- **THEN** pads 1 and 2 are retriggered from the beginning of their effective loops
- **AND** no stop-all command is sent due to the left mouse button action

#### Scenario: Left mouse down restores remembered loops
- **GIVEN** START/STOP previously remembered pads 1 and 2 from a right-button stop
- **WHEN** the performer presses the left mouse button down on START/STOP
- **THEN** pads 1 and 2 are started together
- **AND** START/STOP renders green

### Requirement: Manual Pad Actions Clear Remembered START/STOP Set
The system SHALL clear any remembered START/STOP restore set when the performer manually triggers
or stops a pad after START/STOP has stopped a loop set.

After a manual pad trigger or stop clears the remembered set, START/STOP left mouse down SHALL
restart only the pads that are currently playing. START/STOP right mouse down SHALL remember only
the pads that are currently playing at that moment.

#### Scenario: Manual pad trigger replaces the stopped restore target
- **GIVEN** START/STOP previously remembered pads 1, 2, and 3 from a right-button stop
- **WHEN** the performer manually triggers pad 4
- **AND** the performer presses the left mouse button down on START/STOP
- **THEN** pad 4 is restarted
- **AND** pads 1, 2, and 3 are not started from the old remembered set

#### Scenario: Manual pad stop clears the stopped restore target
- **GIVEN** START/STOP previously remembered pads 1, 2, and 3 from a right-button stop
- **AND** pad 4 is currently playing due to manual pad interaction
- **WHEN** the performer manually stops pad 4
- **AND** the performer presses the left mouse button down on START/STOP
- **THEN** pads 1, 2, and 3 are not started from the old remembered set

### Requirement: START/STOP Right Mouse Stops
The system SHALL make right mouse button down on the bottom-bar START/STOP button stop the
currently playing loop set immediately and remember exactly the pads that were playing at that
moment.

Right mouse button interaction on START/STOP SHALL never start or restore playback, and SHALL not
wait for mouse-button release before stopping. The remembered set SHALL be session-only state and
SHALL NOT be persisted with the project.

#### Scenario: Right mouse down stops and remembers playing loops
- **GIVEN** pads 1 and 2 are currently playing
- **WHEN** the performer presses the right mouse button down on START/STOP
- **THEN** all active audio is stopped immediately
- **AND** pads 1 and 2 are remembered for restore
- **AND** START/STOP renders red

#### Scenario: Right mouse down never starts remembered loops
- **GIVEN** START/STOP previously remembered pads 1 and 2 from a right-button stop
- **WHEN** the performer presses the right mouse button down on START/STOP again
- **THEN** no remembered pad is started due to the right mouse button action
- **AND** START/STOP remains in the stopped state

### Requirement: START/STOP Momentary Output Mute
The system SHALL make a mouse-wheel button hold on the bottom-bar START/STOP button temporarily
mute the audio engine output without changing the persisted Master Volume value.

While the mouse-wheel button is held from START/STOP, the audio-engine output volume SHALL be set
to `0.0`. When the mouse-wheel button is released, the audio-engine output volume SHALL be restored
from the current persisted Master Volume value. The visible Master Volume slider SHALL NOT move due
to this momentary mute. This mute SHALL be implemented as bounded UI/controller work using the
existing audio parameter path and SHALL NOT add disk I/O, Python/GIL access, logging, blocking work,
heavy allocation, neural inference, or any new work to the Rust audio callback.

#### Scenario: Mouse-wheel holding START/STOP mutes output without moving Master Volume
- **GIVEN** Master Volume is `0.7`
- **WHEN** the performer presses and holds the mouse-wheel button on START/STOP
- **THEN** the audio engine receives output volume `0.0`
- **AND** the persisted Master Volume remains `0.7`
- **WHEN** the performer releases the mouse-wheel button
- **THEN** the audio engine receives output volume `0.7`
- **AND** the persisted Master Volume remains `0.7`
