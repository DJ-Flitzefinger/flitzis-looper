## ADDED Requirements

### Requirement: CQT normalization only touches retained transform cells
The offline CQT transform SHALL apply its configured per-octave normalization exactly once to every retained bin/frame cell. A wider octave response caused by rounded downsampling SHALL NOT address discarded columns or alias another row in the trimmed output. The output dimensions and configured filter lengths SHALL retain their existing meaning for ordinary and boundary input lengths.

#### Scenario: Downsampled octaves contain an extra discarded frame
- **GIVEN** valid input at the pinned default geometry with length 8820*k minus one
- **WHEN** octave responses are trimmed and normalized
- **THEN** every retained cell equals its independent unscaled value divided by its analytic filter-length normalization
- **AND** no subsequent row's first column is normalized twice

### Requirement: Tempo window observations do not inherit accumulator state
Each local RCF observation SHALL depend only on the current detector window and its configured weighting. Reusing bounded observation storage SHALL NOT retain the previous window's accumulated or normalized scores. Existing Viterbi temporal smoothing SHALL operate on these independent local observations unchanged.

#### Scenario: A silent window follows different rhythmic predecessors
- **GIVEN** identical current silent detector windows after different prior rhythms
- **WHEN** their local RCF observations are computed with reused storage
- **THEN** each local bank equals the same independent fresh-window reference
- **AND** the complete tempo pipeline still uses the existing temporal decoder

### Requirement: Centered detector windows retain every valid source sample
The tempo tracker SHALL construct centered windows by copying their complete intersection with the detector source and zero-extending only coordinates outside that source. Leading virtual padding SHALL NOT also subtract from the valid source extent. Short input SHALL retain the same centered-window and temporal decoder policy as ordinary input.

#### Scenario: Short detector input has attacks near its tail
- **GIVEN** a 256-frame detector source with valid tail attacks inside both centered windows
- **WHEN** those windows are constructed and analyzed
- **THEN** both local observations include every in-range attack
- **AND** the resulting periods equal independent zero-extended observations through the unchanged temporal decoder
