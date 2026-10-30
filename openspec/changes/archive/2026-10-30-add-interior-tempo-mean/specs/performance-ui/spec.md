## ADDED Requirements

### Requirement: Mean tempo uncertainty is visible in English

PADs whose selected automatic result is a mean SHALL display an English
**Possible BPM drift** indication. BPM details SHALL identify **Mean estimate ·
middle 60%**, approximate constant-tempo behavior and possible drift in long loops.
The indication SHALL describe selected source-current evidence, not a discarded
candidate or proof that physical source variability was detected. Manual/Tap,
busy and replaced-source states SHALL retain their existing evidence guards.

#### Scenario: Selected mean has visible uncertainty
- **GIVEN** a PAD uses a source-current automatic mean
- **WHEN** the PAD grid and BPM details are displayed
- **THEN** the performer can identify its possible drift and mean status in English
- **AND** precise PADs do not acquire the mean warning
