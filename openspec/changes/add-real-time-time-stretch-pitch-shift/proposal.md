# Change: Add real-time time-stretch + pitch-shift playback (dynamic tempo/key)

## Why
The app already exposes a global speed control and has BPM lock / Key lock toggles, but the audio engine currently mixes loops at a fixed rate. The performer needs to change tempo in real-time and have all currently playing loops follow, with optional BPM-locked tempo scaling and key-locked pitch behavior.

## What Changes
- Implement real-time tempo change in the Rust mixer using Signalsmith Stretch (via `signalsmith_dsp`).
- Make global speed changes apply audibly to all currently playing voices in real-time.
- Implement BPM lock and Key lock as audio-affecting modes:
  - BPM lock selects a master BPM based on the current pad (and the current speed), and tempo-matches other pads using per-pad BPM metadata (analysis/manual).
  - Key lock preserves perceived pitch when tempo changes (classic key-lock behavior).
- Add control-plane messaging so the audio thread has the global lock state, master BPM, and per-pad BPM metadata needed to compute per-voice stretch + transpose.
- Update the UI BPM display to show the current effective BPM (instead of a placeholder).

## Impact
- Affected specs:
  - `play-samples` (global speed becomes audible behavior, plus tempo/key lock interaction)
  - `performance-ui` (BPM display and lock controls become behaviorally meaningful)
  - **New**: `time-stretch-pitch-shift` (real-time DSP behavior and constraints)
- Affected systems (expected):
  - Rust audio engine mixer/voices (time-stretch + pitch-shift processing)
  - Rust message protocol between Python and audio thread (new control messages)
  - Python controller state (master BPM selection on BPM lock enable)
  - UI sidebar right BPM display + lock toggles

## Out of Scope
- Beat-phase quantization (e.g., starting loops on the next bar/downbeat) and cross-pad phase alignment.
- Per-pad formant controls or advanced tonality-limit tuning exposed in the UI.
- A separate “set master BPM explicitly” control (this proposal anchors master BPM to the current pad when BPM lock is enabled).

## Clarified Semantics (based on legacy app)
- **BPM lock** maintains a single *master BPM*.
  - When enabled: the app captures master BPM from the current loop as `master_bpm = selected_pad_bpm * current_speed`.
  - While enabled: moving the speed slider updates the master BPM as `master_bpm = selected_pad_bpm * speed`.
  - Per-pad playback uses `speed_for_pad = master_bpm / pad_bpm` when pad BPM is known.
- **Key lock** is classic key-lock: tempo changes do not change perceived pitch. It does not pitch-match pads to a shared musical key.
- Pads without BPM metadata MUST continue to play; under BPM lock they fall back to the plain global speed multiplier.
