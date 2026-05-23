## ADDED Requirements

### Requirement: Immediate Trigger Compatibility Through Transport Scheduler
The system SHALL preserve existing immediate sample trigger behavior when trigger
quantization is disabled.

The implementation MAY internally route immediate triggers through the transport scheduler
with a target equal to the current output frame, or it MAY keep an immediate fast path. In
both cases, `AudioEngine.play_sample(id, velocity)` SHALL retain the current observable
behavior unless the user has explicitly enabled quantized triggering through future controls.

#### Scenario: Existing play_sample remains immediate by default
- **GIVEN** trigger quantization is disabled
- **AND** a sample is loaded into slot `id`
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called
- **THEN** playback starts or restarts using the current immediate trigger behavior
- **AND** no beat or bar boundary wait is introduced

#### Scenario: Missing sample remains safe under the transport path
- **GIVEN** no sample is loaded into slot `id`
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called with quantization disabled
- **THEN** the trigger is ignored or dropped safely
- **AND** the audio callback continues without panic or blocking

### Requirement: Quantized Trigger Failure Does Not Partially Change Playback
When a quantized trigger request cannot be accepted by the fixed-capacity scheduler, the
system SHALL reject the requested scheduled transition without applying partial playback
changes.

For transitions that would stop one or more currently playing pads and start another pad at
a quantized boundary, scheduler rejection SHALL leave currently playing pads unchanged.

#### Scenario: Scheduler-full quantized start leaves playback unchanged
- **GIVEN** trigger quantization is enabled
- **AND** the scheduler is full
- **AND** pad 1 is currently playing
- **WHEN** pad 2 is triggered
- **THEN** the pad 2 start is rejected
- **AND** pad 1 remains playing
- **AND** no partial stop/start transition is applied
