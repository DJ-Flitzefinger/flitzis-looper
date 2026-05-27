# Tasks

- [x] 1.1 Add OpenSpec proposal, design, tasks, and deltas for gain/EQ/metering correction.
- [x] 1.2 Validate the change with `openspec validate correct-gain-eq-metering-architecture --strict`.
- [x] 2.1 Fix manual EQ text entry to accept valid negative values and reject invalid characters before insertion.
- [x] 2.2 Add focused Python tests for negative EQ text entry and clamping.
- [x] 3.1 Add Rust-side master output peak telemetry after pad summing and Master Volume.
- [x] 3.2 Add focused Rust tests for unclamped master peak telemetry and realtime-safe backpressure.
- [x] 4.1 Carry master output peak and one-second clip hold through Python session/controller state.
- [x] 4.2 Add focused Python tests for master peak projection and clip hold.
- [ ] 5.1 Render master output metering in the Master Volume control area.
- [ ] 5.2 Add focused UI/helper validation where available.
- [ ] 6.1 Add focused isolator peak-behavior tests before considering any DSP topology change.
- [ ] 7.1 Consolidate final specs and docs after implementation.
