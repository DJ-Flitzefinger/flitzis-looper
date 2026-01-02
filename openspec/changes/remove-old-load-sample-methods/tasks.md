# Tasks for Removing Old `load_sample` Method

1. **Remove Rust `load_sample` method implementation**
   - Remove the `load_sample` method from `rust/src/audio_engine/mod.rs`
   - Remove the import of `sample_loader` in `mod.rs`
   - Remove the `sample_loader` module include statement in `mod.rs`

2. **Remove Rust `sample_loader` module**
   - Delete `rust/src/audio_engine/sample_loader.rs` file
   - Update `rust/src/audio_engine/mod.rs` to remove the import and reference

3. **Update Python type stub**
   - Remove `load_sample` method signature from `src/flitzis_looper_audio/__init__.pyi`

4. **Update test files**
   - Replace all occurrences of `audio_engine.load_sample()` with `audio_engine.load_sample_async()` in test files
   - Update any test assertions or mocks that reference the old method

5. **Update Python usage examples**
   - Update any Python code that uses `load_sample` to use `load_sample_async`
   - Update UI components and controllers that call `load_sample`

6. **Verify functionality**
   - Ensure all existing tests pass
   - Confirm that `load_sample_async` still works correctly
   - Verify no regressions in audio loading functionality