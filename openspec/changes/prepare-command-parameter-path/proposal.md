# Change: Prepare command and parameter path

## Why

The architecture audit found that discrete playback commands and fast continuous parameter updates
share one control-to-audio queue. The callback now has a bounded drain budget, but a burst of
volume, speed, gain, or EQ updates can still occupy the same queue that trigger and stop commands
need for performance correctness.

This change prepares the bridge for future DSP/FX parameters without implementing a new EQ, a new
DSP effect, or plugin hosting.

## What Changes

- Split current fast parameter updates onto a separate bounded parameter queue.
- Keep discrete playback, loading, transport, stem, loop-region, and mode/state commands on the
  ordered control-command queue.
- Drain command messages before parameter messages in the callback so trigger/stop correctness is
  not blocked by parameter bursts.
- Coalesce drained parameter messages by parameter identity and apply only the latest value per
  parameter during one callback invocation.
- Make existing direct Rust MIDI trigger dispatch all-or-nothing when it needs to enqueue both a
  loop-region update and a play command.
- Document ring-full behavior for command and parameter paths.

## Impact

- Affected specs: `ring-buffer-messaging`.
- Affected docs: `docs/architecture.md`.
- Affected code: Rust message definitions, audio stream callback, Rust input mapping dispatch,
  Python-facing audio parameter setters, focused Rust tests.

## Non-Goals

- No new EQ implementation.
- No new DSP/FX implementation.
- No VST, LV2, CLAP, AU, or other plugin-hosting infrastructure.
- No MIDI latency or jitter rework.
- No Python DSP, GIL access, disk I/O, logging, blocking waits, neural inference, or heavy
  allocation in the audio callback.
