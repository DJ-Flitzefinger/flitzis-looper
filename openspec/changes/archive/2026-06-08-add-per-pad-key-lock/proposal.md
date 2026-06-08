# Change: Add per-pad Key Lock controls

## Why

The current Key Lock performer control is global. Performers need to keep pitch
stable on selected pads while allowing other pads to keep varispeed pitch
behavior during speed and BPM-lock changes.

Per-pad Key Lock must be explicit and durable without weakening the realtime
audio boundary. The existing global Key Lock button remains useful as a fast
master operation, but it should now overwrite each pad's Key Lock intent instead
of being the only effective state.

## What Changes

- Add a per-pad `KEY LOCK` button in the selected-pad left sidebar under the
  Stem Mix / stem controls only for loaded pads.
- Persist a Key Lock boolean for every pad.
- Keep the global `KEY LOCK` button in the right sidebar.
- Change global Key Lock activation so it writes the same enabled value to
  loaded pads only and leaves unloaded pads with no enabled Key Lock intent.
- Allow later per-pad toggles to change only the selected pad after a global
  overwrite.
- Route per-pad Key Lock updates through bounded controller and Rust audio-engine
  APIs.
- Make each active voice choose Key Lock processing from its pad's effective
  Key Lock state.
- Preserve the current Rubber Band backed Key Lock path for pads whose effective
  Key Lock is enabled and the varispeed path for pads whose effective Key Lock
  is disabled.

## Non-Goals

- No removal of the global Key Lock button.
- No per-pad Rubber Band quality controls or backend tuning.
- No plugin hosting, VST/LV2/CLAP/AU scanning, or external plugin lifecycle.
- No realtime stem separation, neural inference, decoding, disk I/O, or JSON
  persistence in the audio callback.
- No change to BPM Lock source selection, global speed range, stem cache
  generation, loop-region editing, trigger quantization, or waveform-editor
  source-domain semantics.
- No migration of callback-internal Rubber Band handles, buffers, paths, or
  backend latency into project persistence.

## Realtime Constraints

The CPAL audio callback and realtime hot path MUST NOT allocate, resize buffers,
perform disk I/O, read or write JSON, call Python or acquire the GIL, call UI
code, block on locks or waits, log, scan or load plugins, run neural inference,
or execute unbounded loops.

Per-pad Key Lock state in the callback must be bounded scalar state, indexed by
pad id, and updated only through fixed-size control messages or equivalent
bounded parameter state.

## Impact

- Affected specs: `performance-ui`, `time-stretch-pitch-shift`,
  `project-persistence`, `load-audio-files`.
- Affected docs: `docs/architecture.md`, `docs/key-lock-backend.md` if
  implementation changes the documented Key Lock ownership details.
- Affected code: Python `ProjectState`, persistence tests, controller global and
  per-pad Key Lock actions, `UiContext` selectors/actions, selected-pad sidebar
  rendering, Rust control messages, PyO3 audio API, mixer Key Lock state, and
  focused Rust/Python tests.
