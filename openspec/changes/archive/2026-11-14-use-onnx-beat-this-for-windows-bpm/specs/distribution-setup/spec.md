## ADDED Requirements

### Requirement: Provision a relocatable Windows BPM worker and CUDA cache

Setup SHALL verify the pinned graph, libsoxr and ONNX Runtime package before
selection. The verified CPU package SHALL support systems without CUDA. On a
usable NVIDIA device, setup SHALL prepare the CUDA cache using the verified
worker with synthetic 48 kHz mono inputs of 30.25, 162 and 8 seconds in order.

#### Scenario: A prepared installation starts again

- **WHEN** the device UUID, driver release, worker, graph, package and warmup
  configuration still match the persistent cache
- **THEN** the installation SHALL reuse the ready cache.

#### Scenario: The driver changes, or the cache is lost

- **WHEN** repair or background analysis detects changed cache identity or loss
- **THEN** one owner SHALL prepare the affected cache and publish readiness only
  after all three CUDA executions and reader lifetimes are confirmed
- **AND** a failure SHALL remain visible while the verified CPU path is available.

#### Scenario: The operator checks resources

- **WHEN** the operator runs a setup or launch resource check
- **THEN** it SHALL report the selected worker, package and CUDA cache state
  without executing inference.
