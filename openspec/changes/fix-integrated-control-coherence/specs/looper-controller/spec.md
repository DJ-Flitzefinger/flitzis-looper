## ADDED Requirements

### Requirement: Synchronous rejected edits preserve coherent durable intent
The system SHALL retain previous durable loop geometry and trigger-quantization intent when the corresponding Native publication is synchronously rejected. No project-changed notification, edit receipt or effective-state claim SHALL represent that rejected edit. A later explicit request for the same desired value SHALL remain eligible for normal guarded publication. Existing asynchronous resident requests SHALL keep their original ACK/rollback owner rather than being canceled or retargeted by a generic synchronous rollback.

#### Scenario: Native refuses a loop-end or quantization edit
- **GIVEN** current project values match the prior admitted intent
- **WHEN** Native raises before admitting a changed loop-end/auto/bars/full-track
  or quantization enable/step/mode request
- **THEN** the prior durable values remain intact
- **AND** a new explicit identical request can still reach Native admission

### Requirement: Acknowledged content geometry and diagnostics follow actual ownership
The existing Pair control mirror SHALL move/swap acknowledged loop geometry with its source content in the same prepared projection as the other residency sidecars. An empty current diagnostic-slot result SHALL mean that no current slot owns the old action; it SHALL NOT fall back to the action's old numeric pad IDs.

#### Scenario: Move or swap is followed by a refused loop edit
- **GIVEN** content has a Native-acknowledged geometry distinct from another pad
- **WHEN** that content moves or swaps and a later loop edit is refused
- **THEN** each content retains its own acknowledged baseline at its current slot

#### Scenario: An old global start fails after its original content was removed
- **GIVEN** the original capture reports no current diagnostic slots
- **WHEN** its terminal failure is observed after replacement content occupies the old pad
- **THEN** the replacement pad receives no diagnostic owned by the removed content
