## ADDED Requirements

### Requirement: Windows HTDemucs setup verifies derived model resources

Windows x64 source, Release and Portable setup SHALL provision the same qualified HTDemucs
worker source, pinned runtime and graphs derived from the byte-verified stock checkpoint.
Setup SHALL validate the exact locally built worker, graphs and runtime before selection.
A Portable Release SHALL resolve the immutable installation for its own built worker.
Product distribution
SHALL keep learned checkpoint bytes within the upstream acquisition and local derivation path.

#### Scenario: A healthy installation is checked

- **WHEN** the HTDemucs package has the verified installed identity
- **THEN** the resource check SHALL confirm that identity without running separation or
  downloading dependencies.

#### Scenario: Provisioning or validation fails

- **WHEN** staging, graph validation or worker qualification fails
- **THEN** setup SHALL preserve the previous usable release and any held Native binary.
