## 1. Specification And Planning
- [x] 1.1 Create the OpenSpec proposal, design, tasks, and spec deltas for Rust-owned transport timing and scheduling.
- [x] 1.2 Update relevant planning documentation without implementing production feature code.

## 2. Transport Timeline
- [x] 2.1 Add a Rust transport timeline module owned by the audio thread.
- [x] 2.2 Track an absolute `u64` output sample-frame clock advanced by rendered frames.
- [x] 2.3 Store validated master BPM and derive beat/bar phase in 4/4.
- [x] 2.4 Add deterministic Rust unit tests for frame advancement, BPM-to-frame conversion, beat phase, bar phase, and boundary behavior.

## 3. Fixed-Capacity Scheduler
- [x] 3.1 Add a fixed-capacity scheduler for absolute output-frame events.
- [x] 3.2 Preserve stable ordering for events with the same target frame.
- [x] 3.3 Define and test late-event behavior.
- [x] 3.4 Define and test scheduler-full rejection without event eviction, blocking, panic, or allocation.
- [x] 3.5 Treat any audio-thread allocation or blocking operation as a blocker before merge.

## 4. Playback Integration
- [x] 4.1 Preserve default immediate `play_sample` behavior when trigger quantization is disabled.
- [x] 4.2 Add quantized pad trigger routing for fixed grid scheduling at `1/16`, `1/32`, and `1/64`.
- [x] 4.3 Ensure MultiLoop disabled stop/start transitions execute atomically at one scheduled frame.
- [x] 4.4 Add Rust mixer/audio-stream tests proving scheduled starts occur at the intended frame offset.
- [x] 4.5 Add Python controller tests only when Python-facing quantization controls are introduced.

## 5. Beatgrid And Downbeat Integration
- [x] 5.1 Publish bounded per-pad timing metadata to Rust using the existing message-passing architecture.
- [x] 5.2 Use first downbeat, first beat, then zero as the onset fallback for pad phase metadata.
- [x] 5.3 Ensure beatgrid/downbeat processing, validation, and allocation remain outside the audio callback.
- [x] 5.4 Add tests for missing metadata and invalid metadata fallback behavior.

## 6. Validation
- [x] 6.1 Run `openspec validate add-rust-transport-timeline --strict`.
- [x] 6.2 Run Rust checks/tests with `uv --no-cache run --no-sync cargo ...`.
- [x] 6.3 Run Python tests/lint/type checks when Python API or controller code changes.

## 7. Settings-Based Quantization Update
- [x] 7.1 Replace the performance-view trigger quantization mode segment with a bottom-bar `Q` toggle.
- [x] 7.2 Move trigger quantization grid selection to Settings with `1/16`, `1/32`, and `1/64` options and `1/32` default.
- [x] 7.3 Keep quantized scheduling on the Rust transport grid and preserve audio callback real-time safety.
- [x] 7.4 Persist and migrate trigger quantization settings.
- [x] 7.5 Align the bottom-bar mode, stem, and Settings controls on a consistent horizontal line.
- [x] 7.6 Render the waveform editor zero-amplitude reference line.

## 8. Permanent Masterclock Repair
- [x] 8.1 Keep the Rust transport timeline running from audio stream initialization, independent of active pad playback.
- [x] 8.2 Stop deriving or refreshing the masterclock from the first-started, oldest active, or currently playing pad during normal quantized triggers.
- [x] 8.3 Change quantized trigger selection to the current or next future global transport grid boundary.
- [x] 8.4 Preserve the effective loop-start source frame for quantized `PlaySample` and `PlaySampleExclusive` starts.
- [x] 8.5 Remove late-click catch-up behavior that starts newly triggered pads inside the loop.
- [x] 8.6 Add MultiLoop regression coverage for stopping any active pad while other pads keep playing.
- [x] 8.7 Re-run strict OpenSpec validation and the full uv-managed Rust/Python validation sequence.

## 9. Shared Master-BPM Bridge
- [x] 9.1 Treat accepted performance master-BPM parameter updates as the shared Rust tempo for transport-grid timing and BPM-lock tempo matching.
- [x] 9.2 Preserve current transport bar phase when applying a new master BPM.
- [x] 9.3 Keep pad-derived transport phase anchoring explicit through `AnchorTransportPhaseFromPad`.
- [x] 9.4 Add focused Rust tests for shared mixer/transport master-BPM application.
