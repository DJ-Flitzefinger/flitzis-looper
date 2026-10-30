## MODIFIED Requirements

### Requirement: Waveform editor supports middle-click playback seek
The system SHALL seek the selected pad's active or paused voice on middle-button
press at the plot time, clamped to that voice's full pinned source extent.

Seek SHALL preserve loop markers, auto-loop state, bar count and grid offset.
A stopped pad SHALL remain stopped and its seek SHALL be a no-op. A paused voice
SHALL remain paused. View-only navigation SHALL remain independent of seeking.

An already prepared resident seek SHALL use existing bounded native publication.
A nonresident seek SHALL prepare a finite complete-track resident exception,
report pending/error separately and retain the previous effective audio/playhead
until matching source/request/window native adoption ACK. Bank replacement SHALL
not change seek bounds of a voice still pinned to the prior source.

#### Scenario: Middle-click seek before loop plays into loop
- **GIVEN** a playing voice loops between 10 and 18 source seconds
- **WHEN** a seek to 5 seconds is ready and acknowledged
- **THEN** it plays the intro to the loop and then loops normally
- **AND** markers remain unchanged

#### Scenario: Middle-click seek inside loop keeps normal wrapping
- **WHEN** a prepared seek to 12 seconds is adopted for that loop
- **THEN** playback resumes there and wraps from 18 to 10 seconds

#### Scenario: Middle-click seek after loop plays to track end
- **GIVEN** that pinned full source lasts 30 seconds
- **WHEN** a prepared seek to 22 seconds is adopted
- **THEN** playback reaches the actual track end before jumping to the loop start

#### Scenario: Middle-click seek does not start a stopped pad
- **WHEN** an inactive and unpaused pad receives a seek
- **THEN** it stays stopped without marker changes or full-track preparation

#### Scenario: Paused seek waits for resident context
- **WHEN** a paused voice seeks outside its resident region
- **THEN** it remains paused while the complete-track exception is prepared
- **AND** only matching adoption updates its effective source position
