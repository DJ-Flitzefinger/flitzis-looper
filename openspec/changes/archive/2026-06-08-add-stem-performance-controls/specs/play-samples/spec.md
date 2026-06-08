## ADDED Requirements

### Requirement: Stem Mix Controls Preserve Pad Voice Timing
The system SHALL apply stem mix mode and future per-stem mask changes without changing pad
voice timing.

Prepared-stem playback SHALL continue to share the same voice playhead, loop region,
transport-scheduled start frame, BPM-lock behavior, key-lock processing, EQ/gain, metering,
and playhead update path as full-mix playback.

#### Scenario: Switching to all-stems keeps the loop position
- **GIVEN** a pad is playing with a valid prepared stem set
- **WHEN** the performer switches the pad from full-mix mode to all-stems mode
- **THEN** playback continues from the same voice playhead position
- **AND** loop-region, BPM-lock, key-lock, EQ/gain, metering, and playhead reporting continue on the same timing path

#### Scenario: Per-stem toggle keeps synchronized playback
- **GIVEN** a pad is playing in all-stems mode
- **WHEN** the performer toggles a selected-pad per-stem control
- **THEN** enabled stems are read from the same loop-relative sample position
- **AND** the pad is not retriggered or time-slipped by the toggle itself

### Requirement: Full-Mix Revert Preserves Playback
The system SHALL allow a pad to revert from prepared-stem playback to full-mix playback
without stopping current playback.

If the full-mix revert request reaches the audio callback, the callback SHALL update bounded
mix state only. It SHALL NOT delete cache artifacts, unload prepared handles, read files,
decode audio, run neural inference, log, block, allocate stem buffers, or acquire the Python
GIL.

#### Scenario: Reverting to full mix is immediate and safe
- **GIVEN** a pad is playing in all-stems mode
- **WHEN** the performer selects full-mix mode
- **THEN** the pad continues playback using the loaded full-mix buffer
- **AND** playback does not require stopping, retriggering, or deleting the prepared stem cache

#### Scenario: Full-mix revert is safe with missing stems
- **GIVEN** a pad has no current prepared stem set
- **WHEN** the performer selects full-mix mode
- **THEN** playback remains on the loaded full-mix buffer
- **AND** the audio callback performs no stem cache file I/O
