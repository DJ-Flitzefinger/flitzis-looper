## MODIFIED Requirements

### Requirement: ONNX Model Inference
The system SHALL load the pretrained KeyNet ONNX model (`keynet.onnx`) and execute inference using the `ort` Rust crate. Each serialized inference call SHALL own its model session and release that session after its input/output views finish, including errors. Independent calls SHALL NOT retain a globally cached session. Global session-access serialization SHALL remain; model resolution, numerical options and trained weights SHALL remain unchanged.

The system SHALL pass the CQT tensor as input to the model and SHALL extract the 24-class logits from the output. The predicted key SHALL be the class with the highest logit value (`argmax`).

#### Scenario: Model session belongs to one complete inference call
- **GIVEN** an eligible key inference call and the resolved KeyNet model
- **WHEN** the call constructs an `ort::Session` and executes inference
- **THEN** it owns the session until its input/output views finish
- **AND** the session is released before another independent call constructs its session
- **AND** subsequent calls load the same resolved model rather than reuse a global session
- **AND** owner release does not certify a particular OS working set or destruction of global ORT runtime resources

#### Scenario: Inference produces 24-class logits
- **GIVEN** a call-owned ONNX session and a valid CQT tensor
- **WHEN** inference is executed
- **THEN** the model outputs a tensor of shape (1, 24)
- **AND** the system identifies the class with the highest logit value
- **AND** the predicted class index is in the range 0 to 23

#### Scenario: Model load failure is handled gracefully
- **GIVEN** the model file is missing or corrupted
- **WHEN** key detection is triggered
- **THEN** the system reports a key detection error
- **AND** the analysis result uses `"unknown"` as the key value
- **AND** the BPM pipeline result is not affected
