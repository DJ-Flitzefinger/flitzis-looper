# pad-button Specification

## Purpose
TBD - created by archiving change refactor-split-controller-uistate-pad-button. Update Purpose after archive.
## Requirements
### Requirement: `_pad_button` is split into focused helpers
The performance pad rendering helper `_pad_button` SHALL be split into focused helper functions (e.g. label formatting, progress overlay, meter drawing, input handling, overlays) to reduce complexity and keep the implementation within Ruff complexity thresholds.

#### Scenario: `_pad_button` complexity is Ruff-compliant
- **WHEN** the developer runs Ruff on the project
- **THEN** `src/flitzis_looper/ui/render/performance_view.py` produces no `C901`, `PLR0912`, `PLR0914`, or `PLR0915` findings for `_pad_button`

### Requirement: Pad rendering behavior is preserved
The `_pad_button` refactor SHALL preserve the current behavior of pad buttons as required by existing specs:
- active pad indication (`performance-ui`, `performance-pad-interactions`)
- loader progress overlay (`async-sample-loading`)
- peak meter rendering (`per-pad-metering`)

#### Scenario: Pad UI behavior remains consistent
- **WHEN** a user interacts with pads (trigger, stop, load, analyze)
- **THEN** the pad button continues to render the same status indicators and respond to clicks as before

