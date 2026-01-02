## 1. Rust: background tasks + analysis integration
- [ ] 1.1 Add `stratum-dsp` dependency to Rust crate (Cargo.toml)
- [ ] 1.2 Add a per-pad background task entrypoint for analysis-only jobs (no decode/resample/channel mapping)
- [ ] 1.3 Add an analysis step in the async sample loading worker **after resampling** (use `AnalysisConfig::default()`)
- [ ] 1.4 Convert decoded/resampled audio to mono for analysis and pass the correct sample rate
- [ ] 1.5 Extend loader/task progress stages to include `Analyzing` with weighted progress
- [ ] 1.6 Extend worker events to deliver BPM, key, and beat grid data to Python

## 2. Python: state and controller wiring
- [ ] 2.1 Add persistent per-pad analysis fields to `ProjectState` (with defaults)
- [ ] 2.2 Add runtime analysis progress/error fields to `SessionState` (if needed)
- [ ] 2.3 Update controller event handling to store analysis results in `ProjectState`
- [ ] 2.4 Ensure unload/replace clears analysis results for that pad

## 3. UI: display and actions
- [ ] 3.1 Render BPM + key in pad top-right when available
- [ ] 3.2 Render BPM + key in the selected-pad sidebar when available
- [ ] 3.3 Rename "Re-detect BPM" to "Analyze audio" in pad context menu and sidebar
- [ ] 3.4 Disable "Analyze audio" while the pad is loading
- [ ] 3.5 Wire "Analyze audio" to trigger analysis-only work and show progress

## 4. Validation
- [ ] 4.1 Add/adjust unit tests for ProjectState JSON serialization including analysis fields
- [ ] 4.2 Add/adjust tests for controller handling of analysis events
- [ ] 4.3 Run Rust tests and Python tests (`pytest`) for the updated behavior
