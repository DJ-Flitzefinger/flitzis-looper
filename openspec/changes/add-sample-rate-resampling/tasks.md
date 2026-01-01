## 1. Implementation
- [ ] 1.1 Add `rubato` dependency to `rust/Cargo.toml`
- [ ] 1.2 Update `decode_audio_file_to_sample_buffer` to perform resampling when needed
- [ ] 1.3 Remove `SampleRateMismatch` error variant from `SampleLoadError`
- [ ] 1.4 Update error handling in `AudioEngine::load_sample` to remove sample rate mismatch case
- [ ] 1.5 Write unit tests for resampling functionality
- [ ] 1.6 Update documentation in `sample_loader.rs`
