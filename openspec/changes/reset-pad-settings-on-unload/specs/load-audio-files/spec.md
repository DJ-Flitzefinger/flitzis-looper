## ADDED Requirements

### Requirement: Unload Resets Track-Bound Pad Settings
The system SHALL reset track-bound per-pad project settings to their default values when audio is
unloaded from a pad.

Track-bound settings SHALL include the pad's sample path, sample duration, analysis result, manual
BPM override, manual key override, Gain/Trim, low/mid/high EQ, loop start, loop end, auto-loop
state, auto-loop bar count, grid offset samples, stem cache metadata, and stem mix preference.

The system SHALL publish neutral live defaults for pad BPM, Gain/Trim, EQ, and loop region outside
the audio callback so a later track loaded into the same pad does not inherit stale live audio
state.

The system MUST NOT reset global project settings, input mappings, selected pad/bank, sidebar
visibility, or other non-track-bound UI preferences as part of unloading a pad.

#### Scenario: Unload clears persisted track settings
- **GIVEN** pad `id` has loaded audio
- **AND** pad `id` has non-default Gain/Trim, EQ, grid offset, loop, manual BPM, and manual key
  settings
- **WHEN** the performer unloads audio from pad `id`
- **THEN** `ProjectState.sample_paths[id]` is `None`
- **AND** the track-bound per-pad settings for `id` are reset to their default values

#### Scenario: Later load starts from pad defaults
- **GIVEN** pad `id` is empty
- **AND** the persisted config still contains stale track-bound settings for `id`
- **WHEN** the performer loads a new audio file into pad `id`
- **THEN** the stale track-bound settings for `id` are cleared before the new load is scheduled
- **AND** the newly loaded track starts from default pad Gain/Trim, EQ, grid offset, loop, BPM, and
  key settings
