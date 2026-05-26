## 1. Specification
- [x] 1.1 Add OpenSpec deltas for dB Gain/Trim behavior, persistence migration, UI layout, metering, and Rust signal placement.
- [x] 1.2 Run official strict OpenSpec validation.

## 2. Python Control And Persistence
- [x] 2.1 Add dB gain helpers, defaults, clamping, formatting, and legacy migration.
- [x] 2.2 Update controllers, restore, input mapping, and type stubs to use dB values.

## 3. Rust Audio Engine
- [x] 3.1 Change the fast pad-gain parameter to carry bounded dB trim targets.
- [x] 3.2 Convert dB to linear gain in Rust and smooth active gain changes.
- [x] 3.3 Apply pad Gain/Trim in the mixer before per-pad EQ/DSP and before master volume.

## 4. UI And Metering
- [x] 4.1 Replace the percent Gain slider with a dB trim control and required mouse gestures.
- [x] 4.2 Render the dB display and horizontal green/yellow meter under the Gain control.
- [x] 4.3 Add clip hold/afterglow state for the selected-pad Gain meter.
- [x] 4.4 Remove the redundant vertical pad-edge level meter while preserving BPM/key pad metadata.

## 5. Tests And Documentation
- [x] 5.1 Add mapping, migration, controller/input mapping, UI helper, metering, and Rust audio tests.
- [x] 5.2 Update architecture/UI documentation for dB Gain/Trim.
- [x] 5.3 Run focused validation and record any remaining gaps.
