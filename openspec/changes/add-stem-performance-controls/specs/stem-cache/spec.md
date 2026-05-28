## ADDED Requirements

### Requirement: Stem Control State Follows Current Source Version
The system SHALL expose stem availability to performer controls only for the pad's current
loaded source version.

If source-version metadata is stale, cache files are missing, cache validation fails, or Rust
publication rejects prepared handles, the control layer SHALL show stems as unavailable or
blocked and SHALL preserve full-mix playback.

#### Scenario: Available state requires matching source version
- **GIVEN** a pad has loaded source version A
- **AND** the project has complete stem cache metadata for source version A
- **WHEN** the performer views stem controls for that pad
- **THEN** the controls may show stems as available

#### Scenario: Source replacement clears stem eligibility
- **GIVEN** a pad has available stems for source version A
- **WHEN** the pad source is replaced with source version B
- **THEN** performer controls no longer expose source version A stems as playable for that pad
- **AND** full-mix playback remains available

### Requirement: Full-Mix And Prepared-Stem Modes Are Explicit
The system SHALL model full-mix playback and prepared-stem playback as explicit per-pad stem
mix modes.

Full-mix mode SHALL use the loaded full-mix buffer. All-stems mode SHALL use the complete
current prepared stem set when available and SHALL degrade to full-mix playback when the stem
set is unavailable, stale, incomplete, failed, rejected, or disabled.

#### Scenario: Full-mix mode ignores available stems
- **GIVEN** a pad has loaded full-mix audio
- **AND** the pad has a complete current prepared stem set
- **AND** the pad stem mix mode is full-mix
- **WHEN** the pad is triggered
- **THEN** playback uses the loaded full-mix buffer

#### Scenario: All-stems mode falls back when cache is unavailable
- **GIVEN** a pad's durable stem mix preference is all-stems
- **AND** no valid current prepared stem set is available
- **WHEN** the pad is triggered
- **THEN** playback falls back to the loaded full-mix buffer
- **AND** the performer can regenerate stems without affecting current playback

### Requirement: Per-Stem Controls Use Bounded Known Stem Kinds
The system SHALL limit per-stem performance controls to bounded masks over known stem kinds and
component-stem presets.

Per-stem control state SHALL be represented as bounded masks or scalar modes, not unbounded
lists or file paths. The bottom-bar `I` preset SHALL mean Drums + Melody + Bass, not the
cached `instrumental.wav` artifact. The `A` preset SHALL mean Vocals + Drums + Melody + Bass and
SHALL NOT add `instrumental.wav` as a fifth audible layer. The audio callback SHALL NOT generate
stems, read cache files, decode audio, allocate stem buffers, run neural inference, log, block, or
acquire the Python GIL in response to per-stem controls.

#### Scenario: Stem toggle updates bounded state only
- **GIVEN** a pad has a current prepared stem set already published to Rust
- **WHEN** the performer toggles the selected-pad vocals stem control
- **THEN** the control update is represented as bounded state for the known stem kinds
- **AND** no stem generation, file I/O, decoding, inference, logging, blocking, heap allocation, or Python/GIL access runs in the audio callback

#### Scenario: Instrumental preset avoids direct instrumental artifact rendering
- **GIVEN** a pad is in all-stems mode with a current prepared stem set
- **WHEN** the performer selects the `I` preset
- **THEN** the runtime enabled-stem mask enables Drums, Melody, and Bass
- **AND** the audio callback does not select only the cached `instrumental.wav` artifact
