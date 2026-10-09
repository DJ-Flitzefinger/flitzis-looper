## ADDED Requirements

### Requirement: Set and retrigger preserve distinct playback semantics
The system SHALL keep SET position-preserving without starting stopped content and SHALL execute SET_RETRIGGER as a fresh normal start-point trigger, including for stopped content and repeated already-active values.

#### Scenario: Same target repeated on stopped and playing content
- **WHEN** SET or repeated SET_RETRIGGER is admitted by mouse or MIDI
- **THEN** SET never starts/seeks; each SET_RETRIGGER starts/restarts from normal loop/sample start
- **AND** retrigger never toggles Play/Stop or disappears because target equals current value

### Requirement: Pitch tuple and attack use one existing scheduled event
The system SHALL apply an admitted pitch tuple and its SET_RETRIGGER attack atomically at the same existing trigger dueframe with current guarded permits and original Quantize/SYNC intent.

Quantized waiting SHALL NOT retune currently playing audio early. Unquantized
offbeats/swing/free attacks SHALL stay immediate. Allfour Quantize/SYNC and
MultiLoop/exclusive semantics SHALL be preserved, with no parallel scheduler or
additional grid. Unsupported/unready/capacity failure SHALL expose atomic pending/
reject without partial pitch/start/stop or rerounding captured input target.

#### Scenario: Quantized pitch waits with its attack
- **WHEN** a pitch+retrigger waits for existing quantized targetT
- **THEN** old effective pitch stays untilT and the same event applies tuple+attack all-or-none
- **AND** unquantized equivalent uses its original free input time

### Requirement: Preparation sharing never replaces accepted attacks
The system SHALL keep each admitted repeated pitch attack as a distinct bounded scheduler record with its own frozen tuple and prepared permit while allowing only preparation work to share or coalesce.

#### Scenario: Rapid equal and opposing attacks while preparation waits
- **WHEN** several attacks are accepted before ACK, including equal values
- **THEN** every accepted event remains ordered and a later pitch request does not globally stale its tuple
- **AND** removed lifetime/source authority still fences unsafe execution; existing unrelated stem-start coalescing remains intact
