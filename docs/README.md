# Looper documentation

| Start here | Scope |
| --- | --- |
| [User guide](user-guide.md) | Performance controls, Settings, editing, stems, recording, and recovery. |
| [Development](development.md) | Source setup, entry points, validation, and OpenSpec workflow. |
| [Windows Portable](portable-windows.md) | Independent installation, updates, repair, and runtime layout. |
| [Model worker setup](model-worker-setup.md) | Beat This, stem models, codecs, and CPU/CUDA configuration. |
| [Architecture](architecture.md) | System map and links to the maintained technical references. |
| [Project TODOs](todos.md) | Explicitly recorded work and links to any temporary implementation plans. |

Consult the [contributor instructions](../AGENTS.md), relevant [OpenSpec contracts](../openspec/specs/),
and affected source and tests before changing behavior. Keep implementation facts
in the relevant architecture chapter. Remove a temporary plan once the verified
result is covered by the maintained references.

## Historical evidence

- [October 30 evidence index](../openspec/changes/archive/2026-10-30-evidence-index.md)
  links the original reports, frozen audit, and numerical and musical evidence.
- [November 13 archive disposition](../openspec/changes/archive/2026-11-13-archive-index.md)
  records the current device and listening acceptance and links the original
  audio-output and performance-isolation checklists and measurements.
- [Source SYNC recovery](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/f241da3d0bdaf2f8a73adc4600f2773a3f9d7522/docs/sync/architecture.md)
  documents the retired source-following and REALTIME BPM work. Current playback
  uses scalar timing.

Archived tasks and reported results retain the scope and status of their runs.
