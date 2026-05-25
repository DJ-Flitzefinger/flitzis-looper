## 1. Implementation

- [x] 1.1 Add controller helpers for converting displayed BPM targets into bounded speed values.
- [x] 1.2 Update right-sidebar Pitch plus/minus and slider movement to use 0.1 BPM steps when an
  effective BPM reference exists.
- [x] 1.3 Render a center-position indicator beside the Pitch control at 1.00x speed.
- [x] 1.4 Add double-click BPM display editing with two-decimal sanitization.
- [x] 1.5 Add focused Python tests for BPM-step conversion and BPM entry sanitization.
- [x] 1.6 Run official OpenSpec validation for `refine-pitch-bpm-control`.
- [x] 1.7 Run focused Python validation and `git diff --check`.
- [x] 1.8 Align the Pitch center indicator to the rendered slider grab geometry and make it green
  only at neutral speed.
- [x] 1.9 Restore the Pitch fader's internal factor display while preserving BPM-grid interaction.
- [x] 1.10 Add hover mouse-wheel and middle-click reset gestures for Pitch, Master Volume, Gain,
  and per-pad EQ controls.
- [x] 1.11 Add held plus/minus repeat behavior for Pitch with bounded 0.1 BPM ticks.
- [x] 1.12 Filter invalid BPM entry characters before they appear in the input field.
- [x] 1.13 Add focused tests for repeat/wheel helper behavior and updated BPM entry filtering.
