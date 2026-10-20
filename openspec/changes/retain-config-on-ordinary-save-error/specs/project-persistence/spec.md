## ADDED Requirements

### Requirement: Ordinary save failures retain current intent and usable retry
Optional debounced and immediate project saves SHALL retain current dirty intent
and keep the GUI usable when an operating-system write error prevents Config
commit. A failed attempt SHALL NOT report a successful save, clear dirty intent,
advance the last successful write timestamp or reset the project. Automatic retry
SHALL wait at least the existing ten-second debounce interval after such a failure
and SHALL use the existing writer to verify and save the latest current intent.
An explicit immediate save MAY retry sooner without a blocking retry loop.

The existing control-thread console sink SHALL report deferred saves without
repeating an unchanged failure on each control poll. Only an actual successful
Config write SHALL clear the retained ordinary save error. Timing verification,
current-revision checks and transaction writer fences SHALL remain authoritative;
raw flush and transactional commit failures SHALL continue to reach their owners.
No save or retry work SHALL enter the realtime audio callback.

#### Scenario: Atomic replacement is temporarily unavailable
- **GIVEN** a valid Config and newer dirty project intent
- **WHEN** an optional save reaches atomic replacement and receives an operating-system error
- **THEN** the optional save SHALL return unsuccessful without aborting the GUI
- **AND** the prior Config bytes, current dirty revision and last successful write timestamp SHALL remain intact
- **AND** only the failed attempt's temporary file SHALL be cleaned up through the existing writer
- **AND** subsequent automatic polls before ten seconds SHALL NOT attempt another write

#### Scenario: Retry saves newer intent and reports a later recurrence
- **GIVEN** an ordinary save failure remains retained and is reported once by the console sink
- **WHEN** project intent changes again and a later save actually succeeds
- **THEN** the existing writer SHALL verify and commit the latest intent rather than replay a failed snapshot
- **AND** actual success SHALL clear the retained save error and its console observation
- **AND** a later occurrence of the same failure SHALL be reported again

#### Scenario: Optional error handling cannot bypass a transaction owner
- **GIVEN** a migration owns the Config writer or timing verification rejects current evidence
- **WHEN** optional saving or a raw transactional commit is attempted
- **THEN** existing timing and writer-fence rejection SHALL remain unchanged
- **AND** an operating-system failure in raw flush or transactional commit SHALL still raise to its existing owner without a false receipt or released fence
