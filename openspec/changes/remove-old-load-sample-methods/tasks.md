# Tasks for Removing Old `load_sample` Method

- [x] 1. Remove Rust `load_sample` method implementation
  - [x] Remove the `load_sample` method from `rust/src/audio_engine/mod.rs`

- [x] 2. Keep Rust `sample_loader` module
  - [x] Keep `rust/src/audio_engine/sample_loader.rs`
  - [x] Ensure `AudioEngine.load_sample_async` uses `sample_loader` for decoding

- [x] 3. Update Python type stub
  - [x] Remove `load_sample` method signature from `src/flitzis_looper_audio/__init__.pyi`

- [x] 4. Update test files
  - [x] Replace all occurrences of `audio_engine.load_sample()` with `audio_engine.load_sample_async()` in test files
  - [x] Update any test assertions or mocks that reference the old method

- [x] 5. Update Python usage examples
  - [x] Update any Python code that uses `load_sample` to use `load_sample_async`
  - [x] Update UI components and controllers that call `load_sample`

- [x] 6. Verify functionality
  - [x] Ensure all existing tests pass
  - [x] Confirm that `load_sample_async` still works correctly
  - [x] Verify no regressions in audio loading functionality
