## 1. Refactoring Preparation
- [x] 1.1 Create `rust/src/audio_engine/` directory structure
- [x] 1.2 Add `mod.rs` with initial module declarations
- [x] 1.3 Extract constants into `constants.rs` (NUM_BANKS, GRID_SIZE, etc.)
- [x] 1.4 Extract error types into `errors.rs` (SampleLoadError)

## 2. Voice Module
- [x] 2.1 Create `voice.rs` with `Voice` struct definition
- [x] 2.2 Move `Voice` struct and implementation from `audio_engine.rs`
- [x] 2.3 Add unit tests for `Voice` behavior

## 3. Mixer Module
- [x] 3.1 Create `mixer.rs` with `RtMixer` struct
- [x] 3.2 Move `RtMixer` implementation from `audio_engine.rs`
- [x] 3.3 Update imports to reference `voice` and `constants` modules
- [x] 3.4 Move mixing tests from `audio_engine.rs` to `mixer.rs`

## 4. Sample Loader Module
- [x] 4.1 Create `sample_loader.rs` with audio decoding logic
- [x] 4.2 Move `decode_audio_file_to_sample_buffer` function
- [x] 4.3 Move `map_channels` utility function
- [x] 4.4 Move `SampleLoadError` error type
- [x] 4.5 Move audio decoding tests to `sample_loader.rs`

## 5. Message Broker Module (Optional)
- [x] 5.1 Create `message_broker.rs` with message handling structures
- [x] 5.2 Extract message processing logic
- [x] 5.3 Add tests for message parsing and handling

## 6. Audio Stream Module
- [x] 6.1 Create `audio_stream.rs` with CPAL stream management
- [x] 6.2 Move stream initialization and callback logic
- [x] 6.3 Extract logger setup into utility function

## 7. Main Module Assembly
- [x] 7.1 Update `mod.rs` to properly export all sub-modules
- [x] 7.2 Update `lib.rs` to use new module structure
- [x] 7.3 Verify all imports are properly qualified
- [x] 7.4 Ensure all tests pass with new structure

## 8. Code Quality and Documentation
- [x] 8.1 Add module-level documentation to each new module
- [x] 8.2 Add intra-doc links between related modules
- [x] 8.3 Run `cargo fmt` to ensure consistent formatting
- [x] 8.4 Run `cargo clippy` and address any warnings
- [x] 8.5 Run `cargo test` to verify all tests pass
- [x] 8.6 Update any in-code documentation that references file structure

## 9. Integration Testing
- [x] 9.1 Run `maturin develop` to verify Python FFI still works
- [x] 9.2 Verify all AudioEngine methods accessible from Python
- [x] 9.3 Run Python tests to ensure integration unchanged

## 10. Final Validation
- [x] 10.1 Verify no public APIs changed
- [x] 10.2 Verify no functionality changes in audio rendering
- [x] 10.3 Verify message passing still works correctly
- [x] 10.4 Verify performance benchmarks remain unchanged
- [x] 10.5 Review diff to ensure only file organization changed

## 11. Cleanup
- [x] 11.1 Remove old `audio_engine.rs` file
- [x] 11.2 Update AGENTS.md if needed to document new structure
- [x] 11.3 Add notes about new module structure for future maintainers
