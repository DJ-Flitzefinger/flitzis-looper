# Design for Removing Old `load_sample` Method

## Architecture Overview

The audio engine currently has both synchronous and asynchronous sample loading capabilities. The synchronous version (`load_sample`) was meant for immediate loading but had blocking behavior that could impact UI responsiveness. The new asynchronous approach (`load_sample_async`) addresses this by performing file I/O operations in background threads.

## Rationale

1. **Redundancy**: Having both synchronous and asynchronous methods creates code duplication and confusion
2. **Performance**: The synchronous method blocks the main thread during file I/O operations
3. **Maintainability**: Simplifying the API reduces maintenance burden and potential bugs
4. **Consistency**: All loading operations should use the same asynchronous pattern

## Implementation Details

### Rust Changes

1. **AudioEngine::load_sample removal**:
   - Remove method implementation from `mod.rs` 
   - Remove import of `sample_loader` module
   - Remove module declaration in `mod.rs`

2. **sample_loader module removal**:
   - Delete the entire `sample_loader.rs` file since it's only used by the old synchronous method
   - Update `mod.rs` to remove references to the module

### Python Changes

1. **Type stub updates**:
   - Remove `load_sample` method signature from `AudioEngine` class in `__init__.pyi`

2. **Test and usage updates**:
   - Replace all test calls to `load_sample` with `load_sample_async`
   - Update any documentation or examples that reference the old API

## Migration Path

The change is backward-incompatible but straightforward for users of the audio engine:

1. Existing code calling `audio_engine.load_sample(id, path)` should be changed to `audio_engine.load_sample_async(id, path)`
2. The return value behavior changes from immediate success/failure to event-based progress reporting
3. Error handling patterns may need adjustment to account for async events

## Testing Strategy

1. Run existing tests to ensure no regressions in `load_sample_async` functionality
2. Update test files to use the new async API instead of the old sync method
3. Verify all audio loading scenarios work correctly with the new implementation