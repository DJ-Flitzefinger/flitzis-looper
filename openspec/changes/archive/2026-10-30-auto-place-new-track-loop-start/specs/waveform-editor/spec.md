## ADDED Requirements

### Requirement: Activity-based grid shares one persisted scalar origin
The system SHALL derive display, snapping and native scalar timing from the
optional persisted source-time base plus manual signed sample offset. Absent base
SHALL retain existing analysis downbeat/beat/zero fallback. An explicit base SHALL
remain independent of BPM, near-start analysis snapping and the offset clamp.

#### Scenario: Activity lies within the legacy near-start snap tolerance
- **GIVEN** a new candidate at loaded frame 1151 at 48 kHz and zero offset
- **WHEN** the grid is displayed and published
- **THEN** its origin equals 1151/48000 seconds rather than zero

### Requirement: Displayed musical coordinates follow the current loop start
The system SHALL display musical beat coordinates relative to current loop start,
whose reference value is 1, without changing physical grid positions. Labels SHALL
use beat distance rather than adaptive subdivision indices. Off-grid starts SHALL
retain fractional coordinates on real grid lines. The start reference SHALL NOT
claim a detected musical beat. Rendering SHALL be bounded and labels thinned for
readability; line 0 and its label SHALL be omitted.

#### Scenario: Later loop renumbers an unchanged grid
- **GIVEN** beats every 0.5 seconds from origin 0.025 seconds
- **WHEN** loop start moves to 4.025 seconds
- **THEN** that line becomes 1 without changing origin or BPM
- **AND** its preceding beat is coordinate 0 and is not drawn

#### Scenario: Manual loop begins halfway between beats
- **GIVEN** Auto is disabled and a loop begins halfway between two beat lines
- **WHEN** coordinates are displayed
- **THEN** neighboring beat lines show 0.5 and 1.5 and the loop reference is 1
- **AND** physical grid lines are not reanchored to manufacture an integer

### Requirement: Invisible line zero defines preceding editor space
The system SHALL start initial/reset and loop-focused views one regular beat
before the loop reference when BPM is valid, including negative source times
when needed. For a new aligned track, the invisible left reference 0 SHALL be
exactly one beat before visible line 1 at the activity/loop boundary. Grid spacing
SHALL retain effective BPM; a short file-start-to-attack interval SHALL NOT be
invented as a full beat. Source-zero access SHALL remain available by navigation.
PCM requests, seeking and physical markers SHALL remain inside loaded audio.
Virtual display space SHALL NOT pad audio, shift stems or alter playback.

#### Scenario: Attack lies close to file start
- **GIVEN** 120 BPM and a new loop/grid boundary at 0.025 seconds
- **WHEN** the editor opens, resets or focuses the loop
- **THEN** the left reference is -0.475 seconds with no drawn line 0
- **AND** visible line 1 is at 0.025 seconds with source audio unchanged

#### Scenario: View lies entirely before the source
- **WHEN** virtual negative time is viewed
- **THEN** grid drawing remains available without a negative PCM read
- **AND** marker/seek gestures cannot address negative audio frames
