## ADDED Requirements

### Requirement: Analysis Device Display Reports Actual Inference Separately From Stage
The UI SHALL display CPU, CUDA or device-unknown from current source/job execution
evidence, separately from generic analysis stage. CUDA fallback SHALL remain
visible. Availability, requested policy and a queued job SHALL NOT imply actual
GPU use. Historical cache entries without an execution receipt SHALL remain
device-unknown; stale, cancelled or replaced-source results SHALL NOT overwrite
current device display.

STEM typed progress SHALL distinguish attempted from confirmed inference device
and preserve its last valid inference receipt through alignment. Original job
subscribers, Move/Copy and completed-cache routes SHALL preserve appropriate
diagnostic provenance without granting artifact publication or audio authority.

#### Scenario: STEM alignment follows CUDA inference
- **WHEN** current owned STEM inference confirms CUDA completion and begins alignment
- **THEN** the UI SHALL show the alignment stage and the retained CUDA inference device separately
- **AND** it SHALL NOT claim that alignment itself executes on CUDA

#### Scenario: A source is replaced during analysis
- **WHEN** an older job later publishes a device receipt
- **THEN** existing source/request/subscriber guards SHALL reject that receipt for the replacement source
