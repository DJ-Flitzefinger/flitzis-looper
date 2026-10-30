## ADDED Requirements

### Requirement: Beat This Setup Is Explicit And Optional
The system SHALL provide an explicit setup operation for the optional isolated Beat This
analysis worker and selected checkpoint, separate from application startup, audio loading,
project restoration and analysis execution. Normal use SHALL NOT silently install packages,
download model weights or substitute another checkpoint/backend.

The worker SHALL use a locked tested interpreter/dependency set and lazy local startup.
CPU operation SHALL be supported by the accepted configuration; CUDA MAY be an explicit
alternative. Documentation SHALL distinguish this optional adapter from current mandatory
Torch/Demucs dependencies, whose removal is a separate work package.

#### Scenario: Ordinary loading never installs an analyzer
- **GIVEN** the optional worker/model is absent
- **WHEN** the application starts, restores a project or loads audio
- **THEN** no analyzer package/model acquisition occurs
- **AND** valid audio and saved analysis remain usable
- **AND** requests for new beat analysis receive explicit unavailable status

#### Scenario: Selected setup is independent of the UI interpreter
- **GIVEN** the user invokes setup for the selected Beat This backend
- **WHEN** the worker environment is provisioned
- **THEN** its exact interpreter and dependencies are recorded and checked
- **AND** compatibility is tested independently of the app's Python version
- **AND** setup does not claim removal of current base Torch/Demucs dependencies

### Requirement: Model Acquisition Records Verified Provenance
The system SHALL identify the selected model by backend/package version, checkpoint name,
content SHA-256, source URL, license evidence and front-end/postprocessor configuration.
Setup SHALL verify the artifact against an accepted manifest and publish it atomically only
after validation. Inference SHALL accept only verified local files and SHALL reject missing,
corrupt or mismatched artifacts before upstream fallback download behavior can execute.

The selected default SHALL be Beat This! 1.1.0 `final0` with minimal postprocessing after its
acceptance/cutover gate. Selecting `small0` SHALL require an explicit configuration with its own
checksum/provenance and acceptance; absence of `final0` SHALL NOT select it automatically.

#### Scenario: Verified model becomes available offline
- **GIVEN** explicit setup has acquired the artifact identified by its accepted manifest
- **WHEN** content hash and configuration validation succeed
- **THEN** the model and provenance are installed atomically
- **AND** later inference can load that local artifact with network access disabled

#### Scenario: Hash mismatch retains the previously verified artifact
- **GIVEN** setup encounters a download whose SHA-256 differs from the accepted manifest
- **WHEN** artifact validation runs
- **THEN** the new artifact is rejected
- **AND** any previously verified installed model remains intact
- **AND** no inference request retries acquisition implicitly
