## ADDED Requirements

### Requirement: Windows automatic BPM uses the pinned ONNX worker

On Windows x64, the system SHALL use the same isolated BeatThis worker for
automatic BPM on import and explicit Analyze. The worker SHALL use the pinned
dynamic FP32 final0 graph with the complete source mono PCM, its digest and
frame extent, and the established frontend and postprocessing.

#### Scenario: CUDA inference and qualified CPU fallback

- **WHEN** the configured worker can execute the graph on a prepared NVIDIA device
- **THEN** it SHALL confirm CUDA execution in its result
- **AND** after a qualified CUDA error it MAY execute one CPU attempt on the same
  source and graph only after CUDA, its process tree, readers and scratch retire.

#### Scenario: Inference fails and an approximate tempo is available

- **WHEN** analysis cannot obtain valid BeatThis predictions
- **THEN** any published approximate BPM SHALL have an empty beat grid and retain
  the analysis failure diagnosis.

#### Scenario: A large complete source is analyzed

- **WHEN** checked complete-source and protocol geometry fit the available host
  memory and staging storage
- **THEN** BPM SHALL admit the preparation without a fixed RAM ceiling
- **AND** cancellation, actual resource pressure or an allocation error SHALL
  settle the request through its existing ownership path.
