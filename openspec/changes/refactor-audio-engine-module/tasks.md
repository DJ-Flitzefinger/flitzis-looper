## 1. Refactoring Preparation
- [ ] 1.1 Create `rust/src/audio_engine/` directory structure
- [ ] 1.2 Add `mod.rs` with initial module declarations
- [ ] 1.3 Extract constants into `constants.rs` (NUM_BANKS, GRID_SIZE, etc.)
- [ ] 1.4 Extract error types into `errors.rs` (SampleLoadError)

## 2. Voice Module
- [ ] 2.1 Create `voice.rs` with `Voice` struct definition
- [ ] 2.2 Move `Voice` struct and implementation from `audio_engine.rs`
- [ ] 2.3 Add unit tests for `Voice` behavior

## 3. Mixer Module
- [ ] 3.1 Create `mixer.rs` with `RtMixer` struct
- [ ] 3.2 Move `RtMixer` implementation from `audio_engine.rs`
- [ ] 3.3 Update imports to reference `voice` and `constants` modules
- [ ] 3.4 Move mixing tests from `audio_engine.rs` to `mixer.rs`

## 4. Sample Loader Module
- [ ] 4.1 Create `sample_loader.rs` with audio decoding logic
- [ ] 4.2 Move `decode_audio_file_to_sample_buffer` function
- [ ] 4.3 Move `map_channels` utility function
- [ ] 4.4 Move `SampleLoadError` error type
- [ ] 4.5 Move audio decoding tests to `sample_loader.rs`

## 5. Message Broker Module (Optional)
- [ ] 5.1 Create `message_broker.rs` if message handling complexity warrants it
- [ ] 5.2 Extract message processing logic
- [ ] 5.3 Add tests for message parsing and handling

## 6. Audio Stream Module
- [ ] 6.1 Create `audio_stream.rs` with CPAL stream management
- [ ] 6.2 Move stream initialization and callback logic
- [ ] 6.3 Extract logger setup into utility function

## 7. Main Module Assembly
- [ ] 7.1 Update `mod.rs` to properly export all sub-modules
- [ ] 7.2 Update `lib.rs` to use new module structure
- [ ] 7.3 Verify all imports are properly qualified
- [ ] 7.4 Ensure all tests pass with new structure

## 8. Code Quality and Documentation
- [ ] 8.1 Add module-level documentation to each new module
- [ ] 8.2 Add intra-doc links between related modules
- [ ] 8.3 Run `cargo fmt` to ensure consistent formatting
- [ ] 8.4 Run `cargo clippy` and address any warnings
- [ ] 8.5 Run `cargo test` to verify all tests pass
- [ ] 8.6 Update any in-code documentation that references file structure

## 9. Integration Testing
- [ ] 9.1 Run `maturin develop` to verify Python FFI still works
- [ ] 9.2 Verify all AudioEngine methods accessible from Python
- [ ] 9.3 Run Python tests to ensure integration unchanged

## 10. Final Validation
- [ ] 10.1 Verify no public APIs changed
- [ ] 10.2 Verify no functionality changes in audio rendering
- [ ] 10.3 Verify message passing still works correctly
- [ ] 10.4 Verify performance benchmarks remain unchanged
- [ ] 10.5 Review diff to ensure only file organization changed

## 11. Cleanup
- [ ] 11.1 Remove old `audio_engine.rs` file
- [ ] 11.2 Update AGENTS.md if needed to document new structure
- [ ] 11.3 Add notes about new module structure for future maintainers