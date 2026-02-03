## 1. Implementation
- [ ] 1.1 Add `PauseSample(id)` and `ResumeSample(id)` variants to the audio engine control message enum (Rust).
- [ ] 1.2 Implement pause/resume logic in the audio callback: pause retains position and stops mixing; resume continues mixing from saved position; ensure real-time safety and no allocations.
- [ ] 1.3 Update Python `AudioEngine` API with `pause_sample(id)` and `resume_sample(id)` methods.
- [ ] 1.4 Modify waveform editor Pause button handler in Python to toggle: if pad is playing, send pause; if paused, send resume.
- [ ] 1.5 Add unit tests for audio engine pause/resume state transitions (Rust).
- [ ] 1.6 Add integration tests for waveform editor pause toggle behavior (Python).
- [ ] 1.7 Run lint, typecheck, and all tests; ensure no regressions.
