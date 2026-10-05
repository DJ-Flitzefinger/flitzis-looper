# Place new-track loop starts at first signal activity

## Why

New assignments always start the loop at source time zero. Leading blank audio
therefore puts the marker before the first useful waveform activity. The user
requested automatic initial placement when loading new tracks, while acknowledging
that an intro, pickup or unrelated sample may precede the musical beat.

## What changes

- Inspect complete immutable loaded PCM in the existing background loader and
  return an exact loaded-frame candidate for initial loop placement.
- Initialize only genuinely new assignments from that candidate, retaining the
  existing 8-bar auto-loop and zero fallback for silence/unavailable metadata.
- Preserve saved/manual regions, BPM, signed grid origin and beat/downbeat analysis.

## Non-goals and realtime constraints

This is signal activity detection, not musical onset/downbeat certification,
Beat This default cutover, variable maps or live SYNC. No trimming or rewriting
of audio, no additional inference, no UI/callback scan and no callback allocation,
blocking, disk I/O or Python/GIL access. Reset/ALL and later manual snapping keep
their current contracts. No optional editor button is added.
