## 1. Implementation
- [x] 1.1 Add `rubato` dependency to `rust/Cargo.toml`
- [x] 1.2 Update `decode_audio_file_to_sample_buffer` to perform resampling when needed
- [x] 1.3 Remove `SampleRateMismatch` error variant from `SampleLoadError`
- [x] 1.4 Update error handling in `AudioEngine::load_sample` to remove sample rate mismatch case
- [x] 1.5 Write unit tests for resampling functionality
- [x] 1.6 Update documentation in `sample_loader.rs`
