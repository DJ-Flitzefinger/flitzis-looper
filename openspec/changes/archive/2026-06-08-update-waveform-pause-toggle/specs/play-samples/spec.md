## ADDED Requirements
### Requirement: Pause and Resume Sample Playback By ID
The system SHALL provide Python APIs to pause and resume playback of a sample by integer `id` without resetting the playhead.

`AudioEngine.pause_sample(id)` SHALL pause playback of the sample associated with `id`, if it is currently playing. Pausing SHALL stop the sample's voice from contributing to the audio output while retaining its current playback position within the loop region (or full sample if no loop region).

`AudioEngine.resume_sample(id)` SHALL resume playback of the sample associated with `id` from its paused position. If the sample was not previously paused, the call SHALL have no effect.

Both functions SHALL be no-ops for `id` with no loaded sample or no active playback voice. They SHALL NOT affect other sample IDs.

#### Scenario: Pause stops mixing but preserves position
- **GIVEN** a sample is loaded into slot `id` and is currently playing
- **AND** playback has progressed to time `t` within the loop region
- **WHEN** `AudioEngine.pause_sample(id)` is called
- **THEN** the sample's voice stops contributing to the audio output immediately
- **AND** the stored playback position remains at time `t`
- **AND** subsequent calls to `resume_sample(id)` will continue from `t`

#### Scenario: Resume continues from paused position
- **GIVEN** a sample in slot `id` is paused with playback position at time `t`
- **WHEN** `AudioEngine.resume_sample(id)` is called
- **THEN** the sample resumes playback from time `t`
- **AND** the voice continues mixing from that point onward

#### Scenario: Pause has no effect if not playing
- **GIVEN** a sample is loaded into slot `id` but is not currently playing (or already paused)
- **WHEN** `AudioEngine.pause_sample(id)` is called
- **THEN** the call succeeds with no effect on playback state

#### Scenario: Resume has no effect if not paused
- **GIVEN** a sample is loaded into slot `id` and is currently playing (not paused)
- **WHEN** `AudioEngine.resume_sample(id)` is called
- **THEN** the call succeeds with no effect on playback state

#### Scenario: Pause/resume are safe for missing sample
- **WHEN** `pause_sample(id)` or `resume_sample(id)` is called for an `id` with no loaded sample
- **THEN** the call is ignored (no exception) and the audio callback continues without panic or blocking
