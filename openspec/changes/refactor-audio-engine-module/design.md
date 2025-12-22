## Context
The current `audio_engine.rs` file has grown to 944 lines, encompassing multiple responsibilities:
- Real-time audio mixing
- CPAL stream management
- Audio file loading and decoding
- Voice management
- Message passing
- Error types and utilities
- All tests

This monolithic structure makes it difficult to:
- Navigate and understand specific subsystems
- Test components in isolation
- Collaborate on different parts of the audio engine
- Add new features without creating more complexity
- Maintain clear separation of concerns

## Goals
1. Create clear module boundaries based on single responsibility principle
2. Maintain 100% API compatibility - no changes to public interface
3. Improve code organization and navigability
4. Separate concerns for easier testing and maintenance
5. Enable parallel development on different audio engine components
6. Make the codebase more approachable for new contributors

## Non-Goals
- Changing any public APIs or behavior
- Modifying the audio processing algorithms
- Changing the message passing protocol
- Performance optimization (unless discovered during refactoring)
- Adding new features

## Decisions

### Decision: Module Structure
Split `audio_engine.rs` into the following module hierarchy:

```
audio_engine/
├── mod.rs              # Main orchestration, re-exports, public API
├── constants.rs        # Configuration constants
├── errors.rs           # Error types (SampleLoadError)
├── voice.rs            # Voice struct and lifecycle
├── mixer.rs            # RtMixer implementation
├── sample_loader.rs    # Audio file decoding
└── audio_stream.rs     # CPAL stream management
```

### Rationale
- **Single Responsibility**: Each module has one clear purpose
- **Logical Grouping**: Related functionality grouped together
- **Testability**: Each module can be tested independently
- **Maintainability**: Easier to locate and modify specific functionality
- **Scalability**: New modules can be added (e.g., effects, different backends)

### Module Responsibilities
- **mod.rs**: Orchestrates sub-modules, maintains AudioEngine public API
- **constants.rs**: NUM_BANKS, GRID_SIZE, NUM_PADS, SPEED_MIN/MAX, VOLUME_MIN/MAX, MAX_VOICES
- **errors.rs**: SampleLoadError enum and conversions
- **voice.rs**: Voice struct with sample_id, frame_pos, volume fields
- **mixer.rs**: RtMixer with load/unload/play/stop rendering logic
- **sample_loader.rs**: decode_audio_file_to_sample_buffer, map_channels
- **audio_stream.rs**: CPAL stream creation, callback setup, logger configuration

### Decision: Visibility and Dependencies
- All sub-modules remain `pub(crate)` - only `mod.rs` exposes public API
- `mod.rs` re-exports necessary types for other parts of the crate
- Constants module has minimal dependencies (just std)
- Error types kept separate for clarity and reusability
- Voice module with minimal dependencies (no CPAL, no ring buffer)

### Rationale
- **Encapsulation**: Implementation details hidden from users
- **Dependency Management**: Clear boundaries reduce coupling
- **Compile Times**: Smaller, focused modules may improve incremental compilation
- **Documentation**: Clearer boundaries for documenting responsibilities

### Decision: Test Organization
Move tests from the main `audio_engine.rs` module into their respective sub-modules:
- Voice tests → `voice.rs`
- Mixer tests → `mixer.rs`
- Decoder tests → `sample_loader.rs`
- Integration tests → Remain in mod.rs
- utilities like `write_pcm16_wav` → Test module where needed

### Rationale
- **Co-location**: Tests near the code they test
- **Isolation**: Easier to test modules independently
- **Maintainability**: Changes to a module only affect its own tests
- **Documentation**: Tests serve as executable documentation for the module's behavior

### Decision: Message Passing Structure
The CPAL audio callback currently handles message processing. This will remain in the audio_stream module as it's inherently tied to the real-time thread.

### Rationale
- Message handling is fundamentally part of the real-time audio thread
- Moving it would add unnecessary complexity and indirection
- Performance critical path should remain clear
- Message processing is not a separable concern from stream management

## Trade-offs and Risks

### Trade-off: Complexity vs Organization
- **Before**: Simple (one file) but unorganized (944 lines)
- **After**: More files to navigate but better organized and documented
- **Mitigation**: Clear module names, good documentation, intuitive structure

### Trade-off: Change Scope vs Benefit
- Risk: Large refactoring with many file changes
- Benefit: Long-term maintainability, easier to add features
- Mitigation: Atomic, incremental commits; extensive testing at each step

### Risk: Breaking Something During Refactor
- Higher risk due to large changes
- Mitigation:
  - Extensive test suite (already exists)
  - Incremental changes (task by task)
  - Continuous validation (cargo test, cargo clippy)
  - Python integration tests after major steps

### Risk: Merge Conflicts
- Refactoring touches many lines and creates new files
- Mitigation: Complete refactoring in dedicated branch, merge promptly

### Trade-off: Imports vs Direct Code
- More imports needed between modules
- Clearer dependencies, better organization
- Modern Rust tooling makes this straightforward

## Migration Plan

### Phase 1: Preparation (Tasks 1.x)
1. Create module structure
2. Extract constants and errors (no behavior changes)
3. Add `mod.rs` with module declarations

### Phase 2: Component Extraction (Tasks 2-6)
4. Extract Voice module
5. Extract Mixer module
6. Extract Sample Loader module
7. Extract Audio Stream module
8. Each extraction validated with tests

### Phase 3: Integration (Tasks 7-8)
9. Wire up all modules in mod.rs
10. Update imports
11. Verify compilation
12. Run all tests

### Phase 4: Validation (Tasks 9-11)
13. Integration testing with Python
14. Performance verification
15. Cleanup old file
16. Documentation updates

### Rollback Plan
- Commit at each phase boundary
- If issues arise, can rollback to previous commit
- Git history preserves entire refactoring process
- Previous `audio_engine.rs` preserved in git history

## Open Questions
1. Should we rename any types during the refactor? **No** - maintain compatibility
2. Should we add more documentation during the refactor? **Yes** - good opportunity
3. Should we update any dependencies? **No** - only structural changes
4. Should we reorganize the tests directory structure too? **No** - scope creep