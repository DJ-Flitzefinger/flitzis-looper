## MODIFIED Requirements


### Requirement: Demucs Quality Parameters Are Bounded
When Demucs is selected, the system SHALL run that backend with bounded quality parameters outside the audio
callback.

The Demucs backend SHALL pass `--shifts 1` and `--overlap 0.5` unless the Settings page
provides validated replacement values. The control layer SHALL persist the configured values in
project state and SHALL reject Demucs quality parameters outside the app-supported range before
invoking Demucs. The app-supported range SHALL be `shifts` from 1 through 20 and `overlap` from
0.25 through 0.95.

#### Scenario: Default quality parameters are used
- **GIVEN** the required Demucs model, FFmpeg tools, and TorchCodec support are available
- **AND** Demucs is the selected separator
- **AND** no Settings page override has supplied alternate quality parameters
- **WHEN** the performer requests stem generation
- **THEN** the background backend invokes Demucs with `--shifts 1`
- **AND** the background backend invokes Demucs with `--overlap 0.5`

#### Scenario: Settings quality values are used
- **GIVEN** the performer configures Demucs shifts to 4
- **AND** the performer configures Demucs overlap to 0.25
- **WHEN** the performer requests stem generation
- **THEN** the background backend invokes Demucs with `--shifts 4`
- **AND** the background backend invokes Demucs with `--overlap 0.25`

#### Scenario: Invalid settings are rejected before Demucs runs
- **GIVEN** the Settings page supplies an unsupported Demucs quality value such as shifts `0`,
  overlap `0.0`, or overlap `1.0`
- **WHEN** the performer requests stem generation
- **THEN** the control layer rejects the request outside the audio callback
- **AND** Demucs is not invoked
- **AND** the pad remains playable with full-mix playback


<!-- From add-offline-stem-cache -->
