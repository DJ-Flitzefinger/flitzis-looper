## MODIFIED Requirements

### Requirement: Pad rendering behavior is preserved
The `_pad_button` refactor SHALL preserve the current behavior of pad buttons as required by
existing specs, except that the redundant vertical right-edge pad level meter SHALL no longer be
rendered inside performance pad buttons.

Pad buttons SHALL continue to show active pad indication, loader progress, pad number, loaded
track label, stem indicators, and available BPM/key metadata overlays. Pad buttons SHALL continue
to respond to trigger, stop, selection, load, and analysis interactions as before.

#### Scenario: Pad UI keeps metadata but omits vertical meter
- **GIVEN** a loaded pad has BPM/key metadata and recent peak telemetry
- **WHEN** the performance pad grid is rendered
- **THEN** the pad button still renders the loaded track label and BPM/key metadata overlay
- **AND** the pad button does not render the previous vertical right-edge level meter

#### Scenario: Pad interactions remain consistent
- **WHEN** a user interacts with pads for trigger, stop, load, or analyze behavior
- **THEN** the pad button continues to render its non-meter status indicators
- **AND** the pad button continues to respond to clicks as before
