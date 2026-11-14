## ADDED Requirements

### Requirement: Windows HTDemucs uses the qualified ONNX worker

On Windows x64 the HTDemucs selection SHALL use the qualified ONNX graph derived from the
pinned upstream checkpoint. The separator SHALL follow the qualified first-stream decoding,
sample-rate and channel conversion, normalization, split, shift, overlap and crop contract.
It SHALL produce the complete aligned four stems and instrumental through the existing
assignment owner.

#### Scenario: CPU is selected

- **WHEN** the captured HTDemucs request selects CPU
- **THEN** the worker SHALL run a qualified CPU arithmetic profile without probing CUDA.

#### Scenario: A qualified CUDA failure occurs

- **WHEN** a captured HTDemucs request permits automatic CUDA and the attempt reports a
  qualified CUDA device or provider failure
- **THEN** the system MAY retry once on qualified CPU with the same sealed decoded input,
  worker weights, shift offsets and effective overlap after the original job, readers and
  scratch retire.

#### Scenario: A new result lacks a verified producer or codec

- **WHEN** a result does not confirm its producer, provider and codec identity
- **THEN** it SHALL remain ineligible for automatic cross-job reuse and calibration.
- **AND** the existing saved-set restoration checks SHALL continue to apply.

#### Scenario: The request is cancelled or its source changes

- **WHEN** cancellation or stale source custody makes the request ineligible
- **THEN** the existing assignment SHALL stay in place and owned pending files SHALL
  retire after their last reader.
