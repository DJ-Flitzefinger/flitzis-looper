# Change: Reset pad settings on audio unload

## Why

After using "Unload Audio", track-specific pad settings can remain in the persisted project config.
Loading a new track into that pad can then inherit old Gain/Trim, EQ, grid offset, loop, BPM, or key
intent that belonged to the previous track.

## What Changes

- Reset track-bound per-pad `ProjectState` fields to their defaults when audio is unloaded from a
  pad.
- Publish neutral live defaults for per-pad Gain/Trim, EQ, BPM, and loop region so a later track in
  the same pad does not inherit stale Rust-side state.
- Treat loading into an already empty pad as a fresh track assignment, clearing any stale
  track-bound config values before scheduling the new load.

## Non-Goals

- No change to global settings such as Master Volume, speed, Multi Loop, BPM Lock, Key Lock, input
  mappings, selected pad/bank, or sidebar visibility.
- No change to sample decoding, stem generation algorithms, or file-copy naming.
- No disk I/O, JSON access, Python/GIL work, blocking waits, logging, neural inference, plugin
  scanning, unbounded loops, or new allocation behavior in the audio callback.

## Impact

- Affected specs: `load-audio-files`
- Affected code: Python loader/controller project-state reset behavior and focused controller tests.
