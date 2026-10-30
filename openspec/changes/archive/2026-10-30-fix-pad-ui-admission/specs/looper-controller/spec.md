## ADDED Requirements

### Requirement: Pad transport GUI admission refusal is visible and nonfatal
The GUI SHALL contain expected Native admission refusal from per-pad stop or waveform stop, reset, seek, pause or resume inside that explicit gesture and present it through the existing pad feedback. The rejected operation SHALL NOT invent a successful projection or reset the playhead after a refused stop. Existing operation ownership, Learn and held-button semantics SHALL remain intact. No later unrelated GUI frame SHALL automatically replay the failed action.

#### Scenario: Waveform stop and reset cannot enter the control queue
- **GIVEN** a loaded active or paused pad with a known playhead
- **WHEN** its GUI stop-and-reset admission raises an expected Native error
- **THEN** the frame remains usable and the pad exposes the error
- **AND** playback projection and playhead do not claim a successful reset

### Requirement: Waveform restart projection follows actual playback feedback
Returning from a waveform restart request SHALL NOT itself clear paused state or replace the effective playhead. Pending, deferred or rejected preparation SHALL retain the previous projection. The existing actual SampleStarted and playhead feedback SHALL remain the authoritative projection path for the new voice.

#### Scenario: Paused waveform restart preparation is pending or rejected
- **GIVEN** an active paused pad and a restart with its original Native input timestamp
- **WHEN** preparation is pending or later rejected without SampleStarted feedback
- **THEN** the old pause and playhead remain visible
- **AND** a successful original start updates projection only through its actual feedback

### Requirement: Performer parameter admission failures stay inside their UI action
Expected Native RuntimeError or ValueError failures from GUI global volume, speed, mute, BPM Lock, quantization and pad gain, EQ or BPM edits SHALL remain visible through the existing global or pad feedback boundary without terminating the GUI. Primary rejection SHALL preserve unadmitted parameter state; later failure SHALL preserve already admitted persistable intent. These boundaries SHALL remain inside the original Learn execute closure, so Learn consumption neither executes Native nor clears prior feedback. TAP return values, original mapped action keys and direct controller exception contracts SHALL remain intact. The boundary SHALL NOT automatically replay the gesture or broadly catch programming errors such as TypeError.

#### Scenario: A slider, button or BPM edit meets Native queue pressure
- **GIVEN** the performer makes one parameter edit whose primary admission is refused
- **WHEN** the GUI action returns
- **THEN** feedback reports the refusal and unadmitted state remains unchanged
- **AND** no frame replay occurs and a later explicit successful retry may clear feedback

#### Scenario: Learn consumes a parameter gesture
- **GIVEN** Learn is active with previous performer feedback
- **WHEN** a learnable parameter gesture selects its original mapping action
- **THEN** the mapping is learned without Native execution or clearing previous feedback

#### Scenario: Derived publication fails after accepted parameter intent
- **GIVEN** Native already admitted the original BPM, speed or Lock intent
- **WHEN** a later derived control publication is refused
- **THEN** feedback is visible while that admitted intent remains persistable
