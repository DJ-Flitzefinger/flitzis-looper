# Change: Tolerate Imperfect MP3 Loads

## Why

Some real-world MP3 files contain incomplete container metadata or isolated damaged frames while
still carrying decodable audio. The current loader fails before decoding when channel metadata is
missing from the track header, and it fails the entire load when Symphonia reports a recoverable
MP3 frame error such as an invalid `main_data` offset.

## What Changes

- Derive source sample rate and channel count from decoded audio buffers instead of requiring that
  metadata to be present on the probed track before decoding.
- Skip isolated recoverable Symphonia decode errors and continue decoding later packets.
- Fail only when no audio frames can be decoded or when the decoded stream changes sample rate or
  channel count mid-stream.
- Preserve non-realtime loading: all file I/O and decoding still run outside the audio callback.

## Non-Goals

- No attempt to repair or rewrite damaged MP3 files on disk.
- No FFmpeg runtime dependency or fallback decoder.
- No support for arbitrary changing channel layouts inside one loaded sample.
- No disk I/O, JSON access, Python/GIL work, blocking waits, logging, neural inference, plugin
  scanning, unbounded loops, or new allocation behavior in the audio callback.

## Impact

- Affected specs: `load-audio-files`
- Affected code: Rust non-realtime sample loader and focused Rust tests.
