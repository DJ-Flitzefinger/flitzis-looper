# Change: Update waveform editor pause button to true toggle behavior

## Why
The waveform editor's Pause button does not function as a proper toggle. Currently, pausing during playback and then pressing Play restarts from the loop beginning instead of resuming from the paused position. This forces users to restart loops even when they want to continue from a paused point. The desired behavior is for the Pause button to act as a true toggle: first press pauses at current position, second press resumes exactly from that position.

## What Changes
- Modify Pause button behavior to toggle between pause and resume without resetting the playhead.
- Ensure Play and Stop buttons remain unchanged (Play always restarts from loop start; Stop always stops and resets to loop start).
- Add pause/resume capabilities to the audio engine's sample playback API (`play-samples`) to support toggling without resetting playback position.
- The change only affects playback of the currently selected pad (no impact on other pads/loops in multi-loop mode).

## Impact
- Affected specs: `waveform-editor`, `play-samples`
- Affected code: Rust audio engine message handling and per-voice playback state; Python UI callback for Pause button; `AudioEngine` API.
