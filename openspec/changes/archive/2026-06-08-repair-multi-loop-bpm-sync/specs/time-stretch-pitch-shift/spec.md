## ADDED Requirements

### Requirement: BPM Lock Active Voice Timing Avoids Cumulative Rounding Drift
The system SHALL render BPM-locked active voice loop phase from the Rust master output timeline, or from a mathematically equivalent non-cumulative timing model, so per-callback integer rounding cannot accumulate audible inter-pad drift.

The repaired timing model SHALL keep source-frame progression, Key Lock processing, and BPM-ratio tempo matching consistent for full-mix and prepared-stem playback. It SHALL NOT require stopping, retriggering, or resetting Rubber Band state during ordinary loop wrapping.

#### Scenario: Callback segmentation does not change BPM-locked phase
- **GIVEN** BPM Lock is enabled with valid per-pad BPM metadata
- **AND** the same output duration is rendered once using fixed callback segments and once using variable callback segments
- **WHEN** active voices reach the same absolute output-frame position
- **THEN** their BPM-locked source loop phase is equivalent in both renders
- **AND** the result does not depend on how prior callback chunks were split

#### Scenario: Key Lock does not introduce a second timing path
- **GIVEN** BPM Lock is enabled
- **AND** Key Lock is enabled
- **WHEN** active voices wrap their loop regions repeatedly
- **THEN** Rubber Band pitch compensation consumes the same repaired source-frame sequence as Key Lock disabled playback
- **AND** Rubber Band output latency does not redefine source loop phase, trigger quantization, or the Rust master output timeline
