## Why

Per-pad stop and waveform transport controls can propagate expected Native
admission errors through the GUI. A waveform restart also changes paused/playhead
projection as soon as its asynchronous request returns, before audio confirms it.

## What Changes

- Route pad and waveform transport admission failures through the existing
  per-pad visible feedback boundary without replay or a second audio path.
- Keep waveform restart playback projection until actual Native playback
  feedback confirms the new voice and position.

## Non-goals

No changes to held right-button pad gestures, Learn semantics, source/timing
guards, global parameter transactions, scheduling or DSP. No realtime work added.
