## Context
- The Rust `AudioEngine` mixes `Voice` instances in a CPAL callback with fixed-capacity arrays.
- Samples are decoded on the Python thread via `AudioEngine.load_sample(id, path)` and published to the audio thread via `ControlMessage::LoadSample`.
- The current mixer plays samples one-shot: voices are removed when they reach the end of the sample buffer.
- The UI already has a per-pad context menu but only with a placeholder **Unload Audio** action.

## Goals / Non-Goals
- Goals:
  - Provide a per-pad **Load/Unload Audio** context menu action (single item) whose label reflects the pad state and either loads audio via a file chooser or unloads the pad.
  - Change playback semantics so `AudioEngine.play_sample(...)` loops continuously until stopped/unloaded.
- Non-Goals (for this change):
  - Stop-all / transport controls
  - Multi-loop mode toggle / one-at-a-time behavior
  - Loop-point editing, BPM workflows, stems, persistence/copying samples to a local directory
  - Bank-specific pad assignments (banks currently affect only UI highlight)

## Decisions
- Loop semantics:
  - `AudioEngine.play_sample(...)` is looping by default; one-shot playback is out of scope for now.
  - Treat the entire decoded sample buffer as the loop region.
  - A triggered voice wraps from end-of-buffer back to start-of-buffer without allocating or blocking.
- API surface:
  - Add `AudioEngine.unload_sample(id)` to complement `load_sample`.
  - `unload_sample` is implemented as a new fixed-size control message (e.g. `UnloadSample { id }`) handled in the audio thread by:
    - stopping active voices for `id`
    - clearing `sample_bank[id]`
- UI workflow:
  - Add a single **Load/Unload Audio** item to the existing per-pad context menu window.
  - The item label is **Load Audio** when the pad has no audio loaded, and **Unload Audio** when the pad has audio loaded.
  - Selecting **Load Audio** opens a Dear PyGui file dialog filtered to `wav`, `flac`, `mp3`, `aif/aiff`, `ogg` and loads the selection into the pad’s sample slot.
  - Selecting **Unload Audio** stops playback and unloads the pad’s sample slot (it does not delete the underlying file).
  - Replacing a pad’s audio is a two-step workflow: unload first, then load.
- Error handling:
  - If loading fails (unsupported format, decode failure, sample-rate mismatch), the app shows a user-visible error dialog and leaves the existing pad contents unchanged.

## Risks / Trade-offs
- Continuous looping means voices can persist indefinitely; in the current engine `MAX_VOICES = 32` can be exhausted if many pads are started without stopping.
  - Mitigation: UI retrigger already stops then plays for the same pad; future work can add “stop all” and/or multi-loop mode.
- Loop boundary clicks/pops are possible for non-seamless audio.
  - Mitigation: out of scope; can add optional fades/crossfades later.

## Migration Plan
- No data migrations: this change only affects runtime behavior and UI.
- If looping semantics are undesirable for certain workflows later, add an explicit loop/one-shot flag per slot and/or separate playback APIs.

## Open Questions
- None.
