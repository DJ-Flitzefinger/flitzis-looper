# Load Audio Files Specification

## Requirements

### ADDED

#### Scenario: Remove synchronous load_sample method
- Given: An existing audio engine with both sync and async loading methods
- When: The change proposal is implemented
- Then: Only the asynchronous `load_sample_async` method should remain available

### MODIFIED

#### Scenario: Update Python type stubs
- Given: Python bindings for AudioEngine
- When: The synchronous load_sample method is removed from Rust
- Then: The Python type stub (`__init__.pyi`) should be updated to remove the `load_sample` signature

#### Scenario: Update test suite
- Given: Existing tests that use `load_sample`
- When: Tests are updated to use `load_sample_async`
- Then: All tests should continue to pass with the new API

### REMOVED

#### Scenario: Remove legacy Rust implementation
- Given: AudioEngine with legacy `load_sample` method
- When: The change is implemented
- Then: The `load_sample` method implementation in Rust should be removed from `mod.rs`
- And: The `sample_loader` module should remain available for async decoding

#### Scenario: Remove Python usage of old method
- Given: Python code that uses the old synchronous API
- When: The change is applied
- Then: All references to `load_sample` should be replaced with `load_sample_async`

## Implementation Details

### Rust AudioEngine Changes:
1. Remove `load_sample` method from `AudioEngine` struct implementation in `src/audio_engine/mod.rs`
2. Keep `src/audio_engine/sample_loader.rs` and use it for decoding in `load_sample_async`

### Python Type Stub Updates:
1. Remove `load_sample` method signature from `AudioEngine` class in `src/flitzis_looper_audio/__init__.pyi`

### Test and Usage Updates:
1. Replace all occurrences of `audio_engine.load_sample()` with `audio_engine.load_sample_async()` in test files
2. Update any documentation or examples that reference the old synchronous API