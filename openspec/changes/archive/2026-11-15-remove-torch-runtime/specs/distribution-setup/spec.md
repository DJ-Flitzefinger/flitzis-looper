## ADDED Requirements

### Requirement: Model Worker Setup Uses The Native ONNX Runtime

Windows x64 source, managed Release and Portable SHALL bind the qualified
native ONNX workers. Stem setup SHALL acquire and verify pinned upstream
inputs in installation-owned or explicitly configured model storage and
derive learned ONNX data locally. Setup SHALL verify the exact worker,
graphs, runtime and CRT before selecting a candidate.

#### Scenario: A prepared installation starts

- **GIVEN** the selected Native application and matching model resources
- **WHEN** the operator checks its start resources
- **THEN** the selected workers and their package identities are verified without
  separation or dependency acquisition
- **AND** each Portable Release uses the immutable HTDemucs installation for
  its sealed worker and CRT

#### Scenario: Setup cannot qualify the candidate

- **WHEN** staging, CPU model loading or worker validation fails
- **THEN** the previous usable installation, Native and user data are preserved
