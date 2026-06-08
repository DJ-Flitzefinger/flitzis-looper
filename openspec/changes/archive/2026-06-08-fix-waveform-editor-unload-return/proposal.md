# Change: Return To Pad View After Editor Unload

## Why

When the waveform editor is open and the performer activates "Unload Audio" for that pad, the
editor stops rendering because the pad is no longer loaded, but the center surface remains assigned
to the editor. This leaves an empty background instead of the pad grid.

## What Changes

- Close the waveform editor session state when audio is unloaded for the pad currently being
  edited.
- Return the center surface to the performance pad view after the edited pad is unloaded.
- Preserve an open waveform editor when audio is unloaded from a different pad.

## Non-Goals

- No change to sample decoding, file deletion, stem cache deletion, pad default reset behavior, or
  project persistence semantics.
- No change to waveform editor transport controls or loop editing behavior.
- No disk I/O, JSON access, Python/GIL work, blocking waits, logging, neural inference, plugin
  scanning, unbounded loops, or new allocation behavior in the audio callback.

## Impact

- Affected specs: `performance-ui`
- Affected code: Python loader session cleanup and focused controller tests.
