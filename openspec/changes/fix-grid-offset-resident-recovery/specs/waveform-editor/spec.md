## ADDED Requirements

### Requirement: Grid and loop gestures survive rejected native admission
The editor and pad GUI SHALL expose synchronous native RuntimeError/ValueError admission
failures through existing pad readiness feedback without terminating the application.
They SHALL restore saved loop-start/end markers after synchronous loop-start publication
rejection and stop a dependent restart when its loop edit fails. A rejected uncaptured start
SHALL NOT be retried or recaptured by polling; a subsequent genuine gesture SHALL obtain
its own original timestamp and native capture. Failure before grid publication SHALL leave
the saved offset and timing intent unchanged. Signed finite grid origins SHALL remain valid;
physical source markers SHALL remain within their existing bounds.

#### Scenario: Grid publication is temporarily unavailable
- **GIVEN** the performer edits Grid Offset and the native queue refuses publication
- **WHEN** the GUI handles that gesture
- **THEN** the existing pad feedback shows the failure and the saved offset remains unchanged
- **AND** other pads continue without an automatic replay of the failed gesture
- **AND** a later successful explicit gesture clears its prior admission error

#### Scenario: Loop edit and play encounters unavailable source timing
- **GIVEN** a loop-editor gesture requests a new loop start and playback
- **WHEN** native source/timing capture is unavailable
- **THEN** the application stays responsive and displays the rejection
- **AND** no playback or exclusive peer stop is fabricated
- **AND** only a fresh user gesture may capture a new start
