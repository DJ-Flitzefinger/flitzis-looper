# Change: Add low-jitter input mapping

## Why
Gen3 input mapping must preserve the existing Learn workflow while removing MIDI-specific
latency and jitter from the normal playback path. Keyboard mapping is currently the
responsiveness reference: it feels direct, while MIDI feels late and uneven even with trigger
quantization set to immediate. That points to the MIDI hot path rather than the Learn UX or the
Rust audio scheduler.

## What Changes
- Preserve the Learn UX: activate `L`, press MIDI or keyboard input, click a learnable action,
  then save the mapping.
- Preserve deletion-by-learn: `L -> Input -> L` deletes that input's mapping.
- Add a Rust MIDI input/control layer outside the audio callback for timestamping,
  normalization, in-memory mapping lookup, and dispatch bridging.
- Stamp MIDI events immediately with monotonic timestamps and filter irrelevant MIDI messages
  before they reach Python/UI work.
- Route mapped MIDI and keyboard inputs to shared LooperAction/command semantics without
  simulating mouse clicks.
- Keep normal playback on in-memory mapping snapshots; JSON is only for UI-owned mapping-file
  edits.
- Add Settings controls for enabling input mapping and deleting all keyboard or MIDI mappings.
- Keep the audio callback protected from MIDI port handling, keyboard polling, JSON access,
  Python/GIL access, blocking locks, logging, and long-running work.

## Impact
- Affected specs: `input-mapping` (new), `performance-ui`, `project-persistence`,
  `ring-buffer-messaging`.
- Affected docs: `docs/architecture.md`, `docs/development.md`.
- Affected code: Rust audio-engine input/control modules, Python input-mapping controller,
  UI context actions, Settings/bottom-bar rendering, persistence models, and tests.

## Non-Goals
- No MIDI output, LED feedback, MPE, aftertouch, pitch bend, SysEx, program change, or MIDI
  clock behavior in version 1.
- No MIDI device selector or mapping editor in this change.
- No direct MIDI-to-audio-callback path.
- No simulated mouse clicks or UI target replay for normal mapped playback.
- No JSON reads/writes, logging, Python/GIL access, or MIDI port handling in the audio callback.
- No real-time stem separation or neural inference in the audio callback.
