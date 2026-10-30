## ADDED Requirements

### Requirement: Explicit project stem audio import
An explicit import SHALL accept the five declared project stem WAV artifacts as input to the existing immutable original capture transaction. The system MUST retain the existing managed-input pin and source-stability checks until capture completes. It SHALL publish an independent canonical original and fresh source/content assignment only after its own Native adoption acknowledgement; it MUST NOT persist a stem-generation path as the PAD original.

#### Scenario: Load a generated stem onto a copied PAD
- **GIVEN** a Re-Arrange copy holds a valid independent assignment and a declared project stem WAV exists
- **WHEN** Load Audio explicitly imports that stem onto the copy
- **THEN** the copy obtains its own acknowledged original capture and remains loaded through later polling, collection and persistence
- **AND** the source sibling and its settings remain unchanged
- **AND** retiring the input stem generation after its actual final reader releases cannot remove the imported original

#### Scenario: Managed stem input is retained during asynchronous capture
- **GIVEN** explicit import has admitted a declared project stem WAV and retained the existing input pin
- **WHEN** its old stem selection requests retirement before the worker finishes capture
- **THEN** physical retirement waits for the protected input while capture and acknowledgement follow the original loader transaction

#### Scenario: Failed stem replacement preserves the previous assignment
- **GIVEN** a copied PAD holds a valid source
- **WHEN** a declared stem WAV cannot be decoded or its input fails existing source-stability checks
- **THEN** the failed import preserves the previous PAD source, content, settings and sibling assignments

#### Scenario: Other managed artifacts do not become originals
- **WHEN** an import names a complete marker, PCM artifact, map, unknown leaf or unsafe managed path, or restore or auto intent names a stem WAV
- **THEN** admission rejects the reference without assignment mutation
- **AND** the explicit stem WAV exception does not grant deletion or direct restore authority
