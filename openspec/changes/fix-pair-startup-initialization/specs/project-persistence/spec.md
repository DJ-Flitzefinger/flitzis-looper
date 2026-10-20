## ADDED Requirements

### Requirement: Pair startup initialization retains a bounded recoverable phase
Pair startup recovery SHALL retain its initialization phase when an expected transient reservation, intent restoration or assignment failure occurs after opening the selected history or capturing its writer. A subsequent poll SHALL attempt at most one initialization advance and reuse every successfully retained writer, history and reservation. No Native handoff SHALL occur before that phase completes. Existing conflict, unknown-history and participant fences SHALL remain authoritative, including edits or replacements made while initialization waits.

#### Scenario: Initialization pressure clears after capturing the writer
- **GIVEN** recognized original or acknowledged-after Pair recovery with a transient initialization failure
- **WHEN** later polling observes that the transient failure has cleared
- **THEN** the original retained recovery advances through its existing source acceptance and durable settlement
- **AND** it creates no duplicate writer, history, reservation or Native transaction

#### Scenario: Pressure persists or a participant is replaced while waiting
- **GIVEN** initialization is pending with its original durable owners
- **WHEN** the next poll still fails or sees changed participant intent
- **THEN** the attempt is bounded and existing evidence remains retained
- **AND** current replacement content is not overwritten or silently captured
