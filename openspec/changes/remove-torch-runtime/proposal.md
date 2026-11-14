# Remove the superseded Python model runtime

## Why

The qualified Windows x64 ONNX workers already supply both stem separators and
automatic BPM. One runtime path per model makes installation and diagnostic
behavior consistent across source, managed Release and Portable.

## Impact

- Remove old Python inference dependencies, workers, preflights and alternate
  routes. HTDemucs uses FFmpeg for decoding and PCM16 WAV encoding.
- Preserve pinned upstream model acquisition and local ONNX weight derivation.
- Use a separator-neutral stem input cache. Check the independently built
  source, managed Release and Portable worker bindings and their CRTs.
- Preserve offline execution, existing source leases and OwnACK boundaries.
  All inference, codec I/O and runtime qualification stay outside realtime
  and the audio callback.

## Non-goals

- Change stored separator settings, model choices, producer schemas, source
  identity, playback adoption or saved-set restore.
- Requalify model numerics or remove historical records and local backups.
