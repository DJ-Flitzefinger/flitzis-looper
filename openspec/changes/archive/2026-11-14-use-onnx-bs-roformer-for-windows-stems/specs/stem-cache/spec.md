## ADDED Requirements

### Requirement: Windows BS-RoFormer generation uses the qualified ONNX worker

On Windows x64 the selected BS-RoFormer MUSDB18HQ separator SHALL execute the
qualified ONNX graph using the effective weights of the pinned original model.
It SHALL process the complete source with the existing release chunk and overlap
geometry, preserving source origin, full final frame and four-stem ordering.
The existing alignment and publication owner SHALL produce the complete
five-file set and independently adopt the assignment after the pad's own ACK.

#### Scenario: CPU policy is requested

- **WHEN** the captured request selects CPU for BS-RoFormer
- **THEN** the system SHALL use the qualified CPU worker without probing CUDA.

#### Scenario: A qualified CUDA device or provider failure occurs

- **WHEN** the captured request allows automatic CUDA and execution reports a
  qualified CUDA device or provider failure
- **THEN** the system SHALL retain the first diagnostic and MAY retry the same
  sealed PCM, effective weights and settings once on CPU after actual process,
  tree and reader completion and retirement of the failed attempt's scratch.

#### Scenario: An attempt loses its current source, is cancelled or fails custody

- **WHEN** cancellation, supersession or an input, package, host, disk or
  integrity failure prevents a complete valid result
- **THEN** the system SHALL maintain the previous assignment and retire owned
  artifacts after the final reader.
- **AND** the system SHALL preserve the failure classification.

#### Scenario: A stem set has no established ONNX producer identity

- **WHEN** a new set lacks verified worker, graph, runtime, provider and
  decoder evidence
- **THEN** it SHALL be excluded from cross-job reuse and provider calibration.
- **AND** explicitly saved assignments SHALL continue to restore through the
  existing source, format, EOF and custody checks.
