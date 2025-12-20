## Context
- The UI is driven from Python (Dear PyGui) and issues control commands to the audio thread via a fixed-capacity ring buffer.
- The Rust audio engine currently mixes looped sample playback at a fixed rate.

## Goals / Non-Goals
- Goals:
  - Provide a performer-friendly global speed control and reset in the performance view.
  - Define and implement the control-plane API + message passing for speed updates.
  - Keep the audio callback real-time safe (no allocations, no blocking, no Python/GIL).
- Non-Goals:
  - Implementing the varispeed DSP/mixing behavior in Rust in this change.
  - BPM lock, master BPM selection, quantized start, tempo sync.
  - Key lock / time-stretching (pitch preservation).
  - Per-pad speed controls.

## Decisions
- Speed representation:
  - Global speed is a `float` multiplier in the range 0.5×..2.0× (default 1.0×).
  - Reset is implemented as setting the multiplier back to 1.0×.
- Message passing contract:
  - Introduce a control message (e.g., `ControlMessage::SetSpeed(f32)`) sent from Python to the audio thread.
  - The audio thread accepts the message and stores the most recent speed value in its real-time state.
  - This change does not require the mixer to apply the stored value to playback.
- Performance-friendly updates:
  - Speed updates are expected to be frequent during slider drags.
  - The `AudioEngine.set_speed(...)` API SHOULD be best-effort if the control ring buffer is full (drop the update rather than raising an exception).

## Follow-up: Rust varispeed implementation
- The actual playback-rate change will be implemented in a separate Rust change proposal.
- That follow-up change will use the specialized DSP/resampling crate signalsmith_dsp.

## Risks / Trade-offs
- This change provides UI and plumbing without audible behavior until the follow-up Rust change ships.
  - Mitigation: keep UI stable and wire it end-to-end so the audio implementation can be dropped in later with minimal churn.

## Migration Plan
- No data migrations. This is runtime-only behavior.
