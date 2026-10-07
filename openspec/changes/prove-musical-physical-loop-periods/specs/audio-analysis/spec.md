## ADDED Requirements

### Requirement: Loop Period Acceptance Separates Musical And Physical Durations
The system SHALL evaluate the accepted musical source period separately from
once-rounded physical loop endpoints and SHALL NOT infer rendered musical
alignment from scalar grid diagnostics or physical-wrap correctness. Compatible
musical loops SHALL retain the intended period without cumulative loop or callback
partition drift; the strict numerical gate SHALL bound unwrapped source-equivalent
phase error to one loaded frame at 75 and 1000 cycles.

#### Scenario: A fractional musical duration rounds to an integer physical loop
- **GIVEN** current source-bound accepted timing and independently rounded physical endpoints
- **WHEN** a compatible loop is repeated for 75 and 1000 cycles at a fractional rate
- **THEN** acceptance compares actual rendered recurrence with the independently computed musical period
- **AND** a cumulative endpoint-rounding discrepancy fails the musical gate even if physical rendering is correct

### Requirement: Rendered Loop Proof Uses Independent Current-Owned Evidence
The system SHALL prove productive source progression and rendered discrete PCM
features with independently computed expectations across fractional periods,
rates, wraps and fixed, irregular and one-frame callback partitions. Evidence
SHALL identify actual current source and acknowledged accepted revision, loaded
rate, physical endpoints, feature policy and the musical/physical result separately.

#### Scenario: A physical-loop renderer matches its PCM oracle
- **GIVEN** immutable source PCM and actual acknowledged current accepted ownership
- **WHEN** the productive mixer renders a loop through different callback partitions
- **THEN** output and measured features are compared against an oracle that does not use productive addressing helpers
- **AND** passing physical sample and partition checks does not suppress a failed musical-period gate

### Requirement: Loop Proof Does Not Invent Audible Or Device Acceptance
The system SHALL keep hardware-free rendered/numerical results distinct from
device, human listening and sustained audible acceptance. Missing evidence or
an unimplemented productive correction SHALL remain an explicit incomplete gate.

#### Scenario: Rendered tests pass without a listening session
- **GIVEN** measured dry mixer output and no actual device or human listening evidence
- **WHEN** G3c results are recorded
- **THEN** device and sustained audible acceptance remain open
- **AND** no inferred listening label or scalar diagnostic marks them complete
