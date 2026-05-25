# Change: Repair Key Lock master-tempo playback

## Why

The performance UI already exposes a toggleable Key Lock button, but the Rust DSP path does not
currently implement the intended master-tempo behavior. The local `cute_dsp::SignalsmithStretch`
wrapper used by the mixer is a simplified placeholder: it scales buffer indices and does not apply
its transpose state to the rendered output. That means the Key Lock toggle is not a reliable
audio-processing mode.

Performers expect DJ-style behavior:

- Key Lock off: Pitch/Speed behaves like varispeed, so tempo and pitch move together.
- Key Lock on: tempo changes continue to affect playback timing, but perceived pitch is preserved
  as closely as practical without stopping or retriggering active loops.

## What Changes

- Repair the Rust mixer-side DSP semantics so Key Lock selects a bounded master-tempo path.
- Keep the implementation on the Rust audio side because Pitch/Speed automation is latency
  sensitive and must combine with BPM Lock, Multi Loop, prepared stems, trigger quantization, the
  master clock, and Loop Editor playhead reporting.
- Replace the placeholder stretch wrapper usage with an allocation-free processing wrapper that
  uses bounded preallocated buffers per voice.
- Expose bounded manual Key Lock DSP parameters in Settings, with the former High preset values as
  the default baseline.
- Preserve the existing ring-buffer control contract: Python still sends only fixed-size scalar
  mode, parameter, and speed updates.
- Document the library decision and future replacement path for a commercial/pro-grade stretch
  backend.

## Non-Goals

- No advanced formant controls, per-pad key controls, or separate pitch-shift target UI in this
  slice.
- No real-time stem separation, neural inference, disk I/O, decoding, logging, blocking waits or
  locks, Python/GIL access, or unbounded allocation in the audio callback.
- No change to trigger quantization, the permanent Rust masterclock, Loop Editor grid anchoring,
  BPM-lock master-BPM selection, or prepared-stem cache generation.
- No proprietary SDK integration in this slice. If a future commercial backend is chosen, it must
  keep the same bounded Rust-side callback contract or run behind a real-time-safe worker/ring
  boundary.

## Impact

- Affected specs: `time-stretch-pitch-shift`, `play-samples`, `ring-buffer-messaging`,
  `performance-ui`.
- Affected docs: `docs/audio-engine.md`, `docs/message-passing.md`,
  `docs/time-stretch-and-pitch-shift.md`.
- Affected code: Rust audio-engine mixer, per-voice DSP wrapper, Python Settings/project state,
  and focused Rust/Python tests.
