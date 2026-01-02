# Change: Remove old `load_sample` method

## Why
The synchronous `load_sample` API blocks the calling thread during file I/O and duplicates the async loading path. Removing it simplifies the API surface and avoids accidental blocking usage.

## What Changes
- Remove the legacy synchronous `AudioEngine::load_sample` method; keep `load_sample_async` as the supported API.
- Update Python bindings/type stubs and tests to stop referencing `load_sample`.

## Impact
- Affected specs: `load-audio-files`
- Affected code: Rust `AudioEngine`, Python bindings/type stubs, and tests that load samples.
