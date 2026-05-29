## ADDED Requirements

### Requirement: Must-apply control publications report enqueue failure
The system SHALL report failed enqueue attempts for must-apply command and parameter publications as caller-visible errors.

Must-apply publications SHALL include startup restore, explicit unload/reset neutralization, and one-shot ordered state changes where Python must know whether Rust accepted the requested live-audio state. A caller MAY deliberately classify high-rate updates as best-effort only when that choice is explicit and test-covered.

#### Scenario: Must-apply parameter update reports full parameter queue
- **GIVEN** a must-apply startup or reset path publishes a global volume, speed, master BPM, per-pad BPM, per-pad gain, or per-pad EQ target
- **AND** the parameter queue cannot accept the message
- **WHEN** the Rust-facing setter is called
- **THEN** the setter returns a caller-visible failure
- **AND** Python does not mark the must-apply publication as accepted

#### Scenario: Must-apply ordered command reports full command queue
- **GIVEN** a must-apply path publishes an unload, loop-region, timing metadata, Key Lock, trigger quantization, or other ordered live-audio state command
- **AND** the ordered command queue cannot accept the message
- **WHEN** the Rust-facing setter is called
- **THEN** the setter returns a caller-visible failure
- **AND** the failure is not hidden behind a successful Python API return

#### Scenario: Best-effort classification is explicit
- **GIVEN** a high-rate control path intentionally treats a superseded update as best-effort
- **WHEN** its queue publication cannot be accepted
- **THEN** the code path documents or exposes that best-effort classification
- **AND** must-apply startup, restore, unload, and reset paths do not reuse the silent best-effort behavior
