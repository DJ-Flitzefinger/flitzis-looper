# Change: Add global speed control (UI + message plumbing)

## Why
The legacy application lets a performer change a global “speed” (playback rate) during performance (roughly 0.5× to 2.0×) and quickly reset back to normal speed (1.0×). Today, Flitzis Looper has fixed-speed loop playback and no performance control surface for speed.

## What Changes
- Add performance UI controls for global speed:
  - Speed slider (0.5×..2.0×, default 1.0×)
  - Reset action (sets speed back to 1.0×)
- Add Python app state for the current global speed and wire UI changes into the app layer.
- Add an `AudioEngine.set_speed(speed: float)` API that sends a speed update to the audio thread via the existing ring buffer message channel.

## Notes / Sequencing
- This change intentionally does **not** implement varispeed DSP in the real-time mixer.
- A follow-up Rust change proposal will implement actual playback-rate changes using a specialized DSP/resampling library (instead of a trivial fractional-index/interpolation approach).

## Impact
- Affected specs: `play-samples`, `performance-ui`
- Affected code (expected): `src/flitzis_looper/app.py`, `src/flitzis_looper/ui.py`, plus minimal Rust/PyO3 plumbing in `rust/src/messages.rs` and `rust/src/audio_engine.rs`
- User-visible behavior: Speed controls appear and update the engine speed setting; audible varispeed behavior lands with the follow-up Rust change.

## Out of Scope
- Implementing speed changes in audio output (deferred to a follow-up Rust change using a specialized library).
- BPM lock and master BPM (listed under the same legacy epic).
- Key lock (time-stretch without pitch change).
