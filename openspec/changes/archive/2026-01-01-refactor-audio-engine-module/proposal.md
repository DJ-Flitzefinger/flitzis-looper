# Change: Refactor Audio Engine Module Structure

## Why
The current `audio_engine.rs` file is approaching 1000 lines, making it difficult to maintain, test, and understand. The monolithic structure combines:
- Real-time mixing logic
- Audio stream management
- File loading and decoding
- Message handling
- Voice management
- Test code

This refactoring will split the module into logical sub-modules while maintaining all existing functionality and APIs.

## What Changes
- **Split `audio_engine.rs` into modular structure**:
  - `audio_engine/mod.rs` - Main module orchestration
  - `audio_engine/mixer.rs` - Real-time mixer (`RtMixer`)
  - `audio_engine/voice.rs` - Voice management (`Voice`)
  - `audio_engine/sample_loader.rs` - Audio file decoding and loading
  - `audio_engine/message_broker.rs` - Ring buffer message handling
  - `audio_engine/constants.rs` - Configuration constants and limits
  - `audio_engine/errors.rs` - Audio-specific error types

- **Module visibility and structure**: Internal refactoring only - no public API changes
- **Test organization**: Move tests to appropriate sub-modules
- **Documentation**: Improved module-level documentation

## Impact
- **Affected specs**: `minimal-audio-engine` spec (currently 1 requirement)
- **Affected code**: `rust/src/audio_engine.rs` → `rust/src/audio_engine/` module structure
- **Breaking changes**: None - internal refactoring only
- **Test coverage**: Maintained and better organized
- **Performance**: No impact - same runtime behavior
- **Maintainability**: Significantly improved organization and code navigation

## Benefits
- Better organization of related functionality
- Easier to understand and modify specific components
- More maintainable test structure
- Clearer separation of concerns
- Better collaboration potential (multiple developers can work on different modules)
- Easier to add new features (e.g., effects, different audio backends)