## ADDED Requirements

### Requirement: Persistent streamed decoding errors cannot certify a complete source
The streamed loader SHALL report a decode failure when consecutive recoverable demux errors exceed its existing bounded tolerance. That limit SHALL NOT be interpreted as EOF, emit successful completion progress or publish the decoded prefix as a complete source manifest. Actual EOF and tolerated errors separated by valid packets SHALL retain the existing codec, PCM and source-bound chronology.

#### Scenario: A valid prefix is followed by 65 consecutive DecodeErrors
- **GIVEN** actual decoded PCM followed by a persistent demux error burst
- **WHEN** the existing consecutive-error tolerance is exceeded
- **THEN** the original transaction reports DecodeError and cannot publish a complete source
- **AND** original source bytes remain untouched and no successful completion is reported

#### Scenario: True EOF or bounded intermittent errors follows valid packets
- **GIVEN** a complete synthetic source and errors within the tolerated consecutive limit
- **WHEN** real decoding reaches actual EOF
- **THEN** complete PCM, original source identity and manifest extent remain correct
