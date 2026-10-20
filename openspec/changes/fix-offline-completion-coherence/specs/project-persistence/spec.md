## ADDED Requirements

### Requirement: Material migration preserves content copy lineage
An acknowledged material-reference migration SHALL preserve the instance identity and copy lineage of each surviving content instance. Updating material identifiers and source references SHALL NOT turn a copied instance into an unrelated original. The candidate, live committed image and durable saved image SHALL agree, and publication SHALL retain the existing actual-source and timing acknowledgement requirements and concurrent-edit handling.

#### Scenario: A committed copied pad migrates with its original
- **GIVEN** a completed Copy transaction and surviving copied content with copied_from
- **WHEN** both references receive matching migration acknowledgement and commit
- **THEN** their source references change through the existing migration transaction
- **AND** the copied instance retains its original copied_from in memory and on disk
