# Change: Add per-pad gain, metering, and 3-band EQ

## Why
Performances need per-pad mixing controls (gain + EQ) and visual level feedback (metering) without compromising real-time safety or UI frame rate.

## What Changes
- Add per-pad gain control exposed in the left sidebar for the currently selected pad.
- Add a lightweight per-pad level meter rendered inside each pad button.
- Add a per-pad 3-band EQ (low/mid/high) controlled from the left sidebar.
- Extend audio-thread → UI telemetry to publish per-pad peak information at a bounded update rate (~10 Hz max).

## Impact
- Affected specs (new): `per-pad-gain`, `per-pad-metering`, `per-pad-eq3`.
- Affected specs (related, no direct delta expected): `ring-buffer-messaging`, `performance-ui`, `play-samples`, `minimal-audio-engine`.
- Affected code (expected): Python UI (`src/flitzis_looper/ui/render/performance_view.py`, `src/flitzis_looper/ui/render/sidebar_left.py`), core/controller state (`src/flitzis_looper/models.py`, `src/flitzis_looper/controller.py`), Rust mixer + message protocol (`rust/src/audio_engine/mixer.rs`, `rust/src/messages.rs`, `rust/src/audio_engine/audio_stream.rs`, `rust/src/audio_engine/mod.rs`).
