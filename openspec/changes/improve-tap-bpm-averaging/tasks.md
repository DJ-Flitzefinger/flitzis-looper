## 1. Implementation

- [x] 1.1 Update `BpmController.tap_bpm` to reset the current series after pauses longer than 3 seconds.
- [x] 1.2 Compute Tap BPM from all intervals in the current measurement series, starting with the second tap.
- [x] 1.3 Keep measurements scoped to explicit Tap BPM activation and the current target pad.
- [x] 1.4 Add focused controller/UI tests for second-tap calculation, full-series averaging, and pause reset.
- [x] 1.5 Run official OpenSpec validation for `improve-tap-bpm-averaging`.
- [x] 1.6 Run focused Python validation and `git diff --check`.
