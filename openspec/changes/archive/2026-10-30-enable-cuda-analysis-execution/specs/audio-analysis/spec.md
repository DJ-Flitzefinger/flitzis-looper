## ADDED Requirements

### Requirement: Selected Analysis Uses A Closed CUDA Auto Execution Profile
New configured analysis SHALL support the known CUDA-capable Python3.12.13 Windows
runtime with Torch/Torchaudio2.8.0+cu128 and the unchanged Beat This1.1.0/final0,
CPU logmel frontend, minimal postprocessor and float32 precision. Actual model
and inference-input device SHALL be verified. TF32 SHALL be disabled. Requested
`auto`, CUDA availability and attempted placement SHALL NOT be reported as
completed execution.

Auto execution SHALL permit at most one fresh CPU model/checkpoint attempt after
CUDA availability/probe/placement/inference failure. The same complete original
PCM/frontend SHALL be reused after failed model/input/logit references retire.
The failed model SHALL NOT be moved back to CPU for retry. CPU frontend,
checkpoint/hash, source/protocol/postprocessor failures, cancellation and parent
timeout SHALL NOT trigger CUDA fallback. Original process admission, cancellation,
deadline, source custody and actual reader retirement SHALL remain in force.

#### Scenario: CUDA inference fails after successful placement
- **WHEN** the known auto worker fails during CUDA inference
- **THEN** it SHALL retire failed model/input/logit references before loading one fresh verified CPU model
- **AND** completed CPU execution SHALL retain the CUDA runtime profile and coded fallback reason
- **AND** failed or cancelled completion SHALL NOT overwrite current execution metadata

### Requirement: Analysis Execution Receipts Preserve Legacy Provenance
The Python bridge SHALL resolve worker configuration once. Native SHALL derive
its expected request from the original Native identity/path/source geometry and
one of the two known profiles, independently of arbitrary response fields. It
SHALL validate the actual encoded request, response model, source metadata and
required execution receipt before accepting protocol two.

The legacy CPU worker project, protocol-one request/result/raw bytes and
scientific reference identity SHALL remain unchanged. New App analyses from that
known CPU-only raw MAY derive a separate diagnostic CPU receipt. Optional schema4
execution metadata SHALL distinguish that receipt from CUDA-profile CPU fallback.
Old cached methods without a receipt SHALL remain device-unknown. Execution
metadata SHALL NOT grant musical, map, timing, phase or hearing acceptance.

#### Scenario: Historical CPU evidence is reopened
- **WHEN** a saved method has no execution receipt
- **THEN** it SHALL remain readable without new fields or a fabricated GPU/CPU confirmation
- **AND** new protocol-two metadata SHALL NOT relabel its original raw evidence
