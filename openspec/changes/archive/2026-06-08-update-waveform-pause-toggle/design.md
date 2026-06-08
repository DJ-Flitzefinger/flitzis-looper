## Context
The waveform editor provides transport controls for the selected pad. The current Pause button only pauses playback; resuming requires pressing Play, which always restarts from the loop start. We need a true toggle behavior: Pause should pause and, when pressed again, resume from the same position. This requires the audio engine to support pausing and resuming sample playback without resetting the playhead.

## Goals / Non-Goals
- Goals: Enable pause/resume toggle via Pause button; maintain real-time safety; keep Play and Stop unchanged; no UI/layout modifications.
- Non-Goals: Changing Play/Stop behavior; affecting other pads; modifying multi-loop mode; adding new UI controls.

## Decisions
- **Message Types**: Add two new control messages: `PauseSample(id: u8)` and `ResumeSample(id: u8)`. Separate messages are simpler than a single toggle that infers state.
- **Voice State**: Each voice will have a `paused: bool` flag along with its current playback position. When paused, the voice's `paused` flag is set to true; the audio callback will skip mixing for paused voices but keep the read position unchanged. When resumed, the flag is cleared and mixing continues from the saved position.
- **Audio Thread Safety**: Pause/resume operations will set this flag via atomic write or under a lock-free scheme. The voice struct can have an `AtomicBool` for paused, or a plain bool with careful ordering (since messages are processed sequentially by the audio thread, writing to the voice's paused field in the message handler is safe).
- **UI State**: The Python UI will maintain a local `is_paused` state per pad (or derive it from playback state). When the Pause button is pressed, it checks this state and sends the appropriate message. The state can be updated upon receiving messages or via feedback from the audio engine if needed.
- **Stop vs Pause**: `StopSample(id)` (existing) stops the voice and frees it; pause retains the voice but mutes it. A paused voice can be resumed; a stopped voice must be re-triggered with `play_sample`.
- **Loop Region**: Playback position respects the configured loop region; pausing within the loop and resuming continues within the loop without jumping.

## Risks / Trade-offs
- **Memory**: Adding a boolean per voice (32 max) is negligible.
- **Real-Time Safety**: The message processing must be lock-free and non-allocating. Updating a bool in the voice struct is safe as the audio thread owns the voices during processing. The control message will contain the ID and a flag; the audio thread will look up the voice and set the flag. No allocations.
- **Edge Cases**: If a voice is paused and then `stop_sample` is called, the voice should be terminated normally (paused flag cleared). If a voice finishes its loop naturally while paused, the position should remain at the loop end? Probably fine.

## Migration Plan
This is a behavior change only; no data migration needed. Projects will automatically use the new behavior upon update.

## Open Questions
- Should we expose separate `pause_sample` and `resume_sample` methods or a combined `toggle_pause_sample`? We'll expose separate methods for clarity, but the UI can implement toggle.
- Should the audio engine provide playback state queries? Not needed; UI can track locally.
