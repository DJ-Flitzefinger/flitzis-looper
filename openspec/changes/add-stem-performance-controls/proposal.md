# Change: Add performer-facing stem controls and persistence planning

## Why
The offline stem cache work now has project-local cache identity, background generation,
prepared stem publication, and prepared-stem rendering fallback infrastructure. Performers
still have no specified way to generate stems from the UI, see availability/progress, choose
between full-mix and prepared-stem playback, or persist durable stem control preferences.

This change defines that control surface before implementation so the UI does not promise
behavior that the real-time audio path cannot safely provide.

## What Changes
- Define performance UI stem availability indicators for unavailable, generating, available,
  blocked, and error states.
- Define selected-pad action entry points for generating stems, selecting full mix versus all
  prepared stems, and bottom-bar per-stem mask controls.
- Implement a selected-pad bottom-bar `V`, `D`, `M`, `B`, `I`, `A` control cluster where `I`
  means the instrumental preset Drums + Melody + Bass and `A` means Vocals + Drums + Melody + Bass.
- Define and implement project persistence expectations for stem cache metadata and durable
  full-mix/all-stems stem mix preferences while keeping momentary performance gestures transient.
- Require audio-thread stem mix state changes to use bounded fixed-size control messages,
  including per-pad enabled-stem masks.
- Preserve full-mix playback as the safe fallback whenever stems are unavailable, stale,
  incomplete, rejected, or disabled.
- Keep real-time stem separation, neural model inference, disk I/O, logging, blocking,
  heap allocation, long-running work, and Python/GIL access out of the audio callback.

## Impact
- Affected specs: `performance-ui`, `project-persistence`, `stem-cache`,
  `play-samples`, `ring-buffer-messaging`, and `background-tasks`.
- Affected docs: `docs/audio-engine.md`, `docs/message-passing.md`,
  `docs/todos-legacy-migration.md`.
- Later affected code: `ProjectState`, UI selectors/actions, selected-pad sidebar, pad-grid
  indicators, stem controller actions, Rust control messages, bounded mixer stem mix state,
  and focused Rust/Python tests.

## Non-Goals
- No complete performer-facing UI implementation in the first implementation slice.
- No neural source-separation model selection, dependency installation, model download, or
  production stem quality change.
- No real-time stem separation.
- No direct use of the cached `instrumental.wav` artifact as the `I` button or as an additional
  fifth audible layer for the `A` button.
- No durable persistence of per-stem mask gestures in this slice.
- No disk I/O, decoding, neural inference, logging, blocking, heap allocation, long-running
  work, or Python/GIL access in the audio callback.
- No replacement of the existing `rtrb` ring-buffer message-passing architecture.
- No changes to Gen3 transport quantization, BPM-lock phase anchoring, loop-region editing,
  or waveform-editor transport behavior.
