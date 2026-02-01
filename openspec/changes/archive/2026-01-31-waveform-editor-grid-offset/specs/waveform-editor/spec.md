## ADDED Requirements

### Requirement: Waveform editor provides a per-pad Grid Offset control
The waveform editor SHALL provide a "Grid Offset" knob/control in its toolbar.

The control SHALL be placed to the right of the current right-most control in the toolbar and SHALL be sized consistently with the existing toolbar controls.

The Grid Offset value SHALL be expressed and displayed as a signed integer in samples (`grid_offset_samples`).

The Grid Offset value SHALL be stored per pad. If no stored value exists for a pad (e.g., older projects), `grid_offset_samples` SHALL default to 0.

**Interaction**
- Left-click dragging the control SHALL adjust `grid_offset_samples` in fine steps of 1 sample.
- Right-click dragging the control SHALL adjust `grid_offset_samples` in coarse steps of 10 samples.

#### Scenario: Default grid offset is zero for an uninitialized pad
- **GIVEN** a pad is loaded from a project that does not contain a stored `grid_offset_samples`
- **WHEN** the waveform editor is opened for that pad
- **THEN** the Grid Offset control displays 0 samples

#### Scenario: Dragging adjusts grid offset in fine vs coarse steps
- **GIVEN** the waveform editor is open for a pad
- **WHEN** the performer left-click drags the Grid Offset control
- **THEN** the `grid_offset_samples` value changes in 1-sample steps
- **WHEN** the performer right-click drags the Grid Offset control
- **THEN** the `grid_offset_samples` value changes in 10-sample steps
